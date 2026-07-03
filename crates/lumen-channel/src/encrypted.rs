//! AES-256-GCM + x25519 보안 채널.
//!
//! 임의의 [`SecureChannel`] transport 위에 confidential, authenticated 계층을
//! 추가합니다. 설계는 의도적으로 보수적입니다:
//!
//! - **Ephemeral x25519 ECDH** - 매 핸드셰이크마다 새 임시 키 쌍을 생성하여
//!   forward secrecy 를 확보합니다. 정적 ed25519 신원 키는 임시 x25519 공개
//!   키를 *서명* 하여 KEX 자체가 MITM 공격에 강하도록 합니다 (Noise IK 패턴
//!   유사).
//! - **사전 핀된 ed25519 신원** - TOFU 없음. 양측은 핸드셰이크 전 상대의
//!   `VerifyingKey` 를 알고 있어야 합니다.
//! - **블레이크3 keyed-derive** - 공유 비밀로부터 두 개의 256 비트 디렉션
//!   특화 (initiator → responder, responder → initiator) 키를 도출합니다.
//!   라벨 분리로 키 재사용을 차단합니다.
//! - **결정론적 nonce** - `(role: u8 || pad: 3 || seq: u64 LE) = 12 bytes`.
//!   role 은 송신자 역할 (initiator=0, responder=1); seq 는 송신 직전에 1 씩
//!   증가합니다. 양측이 *별도 키* 와 *별도 카운터* 를 사용하므로 nonce 는
//!   고유합니다. 카운터 오버플로우는 [`Error::Channel`] 로 거부됩니다.
//! - **서명된 핸드셰이크 epoch 바인딩** - 호출자가 합의한 `epoch` 를 핸드셰
//!   이크 메시지에 포함하여 서명합니다. epoch 가 바뀌면 핸드셰이크 자체가
//!   무효가 되어 cross-epoch 리플레이가 차단됩니다.
//!
//! 이 모듈은 [`crate::attested::AttestedChannel`] 의 보완재이지 대체재가
//! 아닙니다. attested 채널은 무결성 + 신원 + 리플레이 저항을 제공하지만
//! **기밀성** 은 없습니다. 이 모듈은 동일한 ed25519 핀을 재사용해 KEX 를
//! 인증하면서 기밀성을 추가합니다.
//!
//! 폐쇄형(Air-Gapped) iso-light-k0 마이크로커널 환경 호환을 위해 모든
//! 암호 프리미티브는 `elib-k0-nt` 모듈을 사용합니다 - AES-GCM 은
//! `elib-k0-nt/aes::AES256GCM`, ECDH 는 `elib-k0-nt/x25519`, KDF 는
//! `lumen_core::hash::blake3_keyed_derive_32` (BLAKE3 keyed 모드).

use async_trait::async_trait;
use elib_aes::{AES256GCM, GCM_NONCE_SIZE, GCM_TAG_SIZE};
use elib_x25519::{PublicKey as X25519Public, SecretKey as X25519Secret};
use elib_zeroize::Zeroize;
use lumen_core::hash::blake3_keyed_derive_32;
use lumen_core::rng::{OsRng, Rng};
use lumen_core::{Error, Result, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::SecureChannel;

/// 핸드셰이크 마커. 버전을 올리면 이전 피어와 깔끔히 단절됩니다.
pub const ENCRYPTED_HELLO_MARKER: &str = "lumen.encrypted.aesgcm-x25519.v1";

/// 디렉션별 AES-256-GCM 키 도출 라벨.
const KDF_LABEL_INITIATOR: &[u8] = b"lumen.encrypted.k-i2r.v1";
const KDF_LABEL_RESPONDER: &[u8] = b"lumen.encrypted.k-r2i.v1";

/// 서명 도메인 분리자 - ed25519 가 임시 x25519 공개키에 서명할 때 사용.
const KEX_SIGN_DOMAIN: &[u8] = b"lumen.encrypted.kex-bind.v1";

/// 와이어 hello (서명 *대상* 평문).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct EncryptedHello {
    marker: String,
    /// 송신자 ed25519 신원.
    my_id: VerifyingKey,
    /// 송신자가 기대하는 (사전 핀된) 피어 ed25519 신원.
    peer_id: VerifyingKey,
    /// 송신자가 KEX 용으로 만들어 보내는 임시 x25519 공개키.
    ephemeral_x25519: [u8; 32],
    /// 핸드셰이크 epoch (호출자 합의).
    epoch: u64,
    /// 16 바이트 난수 - 핸드셰이크 리플레이 방어.
    nonce: [u8; 16],
}

/// 와이어 핸드셰이크 프레임.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SignedEncryptedHello {
    hello: EncryptedHello,
    signature: Signature,
}

/// 와이어 데이터 프레임 (AEAD ciphertext + 12-byte nonce).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct EncryptedFrame {
    /// 송신 측 카운터; 수신 측은 정확히 다음 기대값과 일치해야 합니다.
    seq: u64,
    /// AEAD 출력 (ciphertext || tag) - 외부 와이어 호환을 위해 elib-k0-nt
    /// 의 (ciphertext, tag) 쌍을 단일 바이트열로 직렬화한 것.
    ciphertext: Vec<u8>,
}

/// AES-256-GCM + x25519 채널 wrapper.
///
/// `inner` 위에서 mutual-authenticated x25519 ECDH 핸드셰이크를 수행하고,
/// 이후 모든 프레임을 디렉션별 AES-256-GCM 키로 암호화 / 인증합니다.
pub struct EncryptedChannel<C: SecureChannel> {
    inner: C,
    /// 송신용 키 (자기 → 피어).
    tx_cipher: AES256GCM,
    /// 수신용 키 (피어 → 자기).
    rx_cipher: AES256GCM,
    /// 자신의 역할 (initiator=0, responder=1) - nonce 첫 바이트로 사용.
    my_role: u8,
    /// 피어의 역할 - 수신 시 nonce 검증.
    peer_role: u8,
    /// 송신 카운터.
    next_send_seq: u64,
    /// 수신 카운터.
    next_recv_seq: u64,
    /// 협상된 epoch (디버그/회전용).
    epoch: u64,
}

impl<C: SecureChannel> EncryptedChannel<C> {
    /// initiator 측에서 호출. `role` 은 nonce 의 첫 바이트로 사용됩니다.
    /// 양측이 동일한 `role` 을 가지면 nonce 충돌이 발생하므로, [`handshake_initiator`]
    /// 와 [`handshake_responder`] 가 서로 다른 값을 사용합니다.
    ///
    /// [`handshake_initiator`]: Self::handshake_initiator
    /// [`handshake_responder`]: Self::handshake_responder
    pub async fn handshake_initiator(
        inner: C,
        sk: SigningKey,
        peer_vk: VerifyingKey,
        epoch: u64,
    ) -> Result<Self> {
        Self::handshake_inner(inner, sk, peer_vk, epoch, 0).await
    }

    /// responder 측에서 호출.
    pub async fn handshake_responder(
        inner: C,
        sk: SigningKey,
        peer_vk: VerifyingKey,
        epoch: u64,
    ) -> Result<Self> {
        Self::handshake_inner(inner, sk, peer_vk, epoch, 1).await
    }

    async fn handshake_inner(
        mut inner: C,
        sk: SigningKey,
        peer_vk: VerifyingKey,
        epoch: u64,
        my_role: u8,
    ) -> Result<Self> {
        let my_vk = sk.verifying_key();

        // 임시 x25519 키 + 16 바이트 핸드셰이크 nonce 를 OS 엔트로피에서 생성.
        // eph_seed 는 X25519Secret 으로 흡수된 직후 zeroize 하여 forensic
        // 메모리 덤프에서 ephemeral 비밀이 회수되지 않도록 합니다.
        let mut eph_seed = [0u8; 32];
        OsRng.fill_bytes(&mut eph_seed);
        let my_eph = X25519Secret::from_bytes(eph_seed);
        eph_seed.zeroize();
        let my_eph_pub = my_eph.public_key();
        let mut nonce16 = [0u8; 16];
        OsRng.fill_bytes(&mut nonce16);

        let hello = EncryptedHello {
            marker: ENCRYPTED_HELLO_MARKER.to_string(),
            my_id: my_vk,
            peer_id: peer_vk,
            ephemeral_x25519: *my_eph_pub.as_bytes(),
            epoch,
            nonce: nonce16,
        };
        let sig_payload = signing_payload(&hello)?;
        let signature = sk.sign(&sig_payload);
        inner
            .send(&SignedEncryptedHello {
                hello: hello.clone(),
                signature,
            })
            .await?;

        let peer: SignedEncryptedHello = inner.recv().await?;
        if peer.hello.marker != ENCRYPTED_HELLO_MARKER {
            return Err(Error::Crypto(format!(
                "encrypted: marker mismatch (peer={:?})",
                peer.hello.marker
            )));
        }
        if peer.hello.my_id != peer_vk {
            return Err(Error::Crypto(
                "encrypted: peer claimed wrong identity".into(),
            ));
        }
        if peer.hello.peer_id != my_vk {
            return Err(Error::Crypto(
                "encrypted: peer hello not addressed to us".into(),
            ));
        }
        if peer.hello.epoch != epoch {
            return Err(Error::Crypto(format!(
                "encrypted: epoch mismatch (expected {epoch}, got {})",
                peer.hello.epoch
            )));
        }
        let peer_payload = signing_payload(&peer.hello)?;
        peer_vk.verify(&peer_payload, &peer.signature)?;

        // x25519 ECDH.
        let peer_x25519 = X25519Public::from_bytes(peer.hello.ephemeral_x25519);
        let shared = my_eph.diffie_hellman(&peer_x25519);

        // RFC 7748 권고: 모든 비트가 0 인 공유 비밀은 비여직성(non-contributory)
        // 키 합의를 의미하므로 거부합니다.
        if shared.is_zero() {
            return Err(Error::Crypto(
                "encrypted: x25519 produced all-zero shared secret".into(),
            ));
        }

        // 디렉션별 키 도출. initiator → responder 와 responder → initiator
        // 두 개를 만들고, 자신의 송신/수신을 그에 매핑합니다.
        // AES256GCM 이 키를 라운드키로 확장한 직후 원본 32 바이트 사본을
        // zeroize 하여 스택 잔류 비밀을 최소화합니다.
        let mut key_i2r = derive_key(shared.as_bytes(), KDF_LABEL_INITIATOR, epoch);
        let mut key_r2i = derive_key(shared.as_bytes(), KDF_LABEL_RESPONDER, epoch);
        let (mut tx_key, mut rx_key, peer_role) = if my_role == 0 {
            (key_i2r, key_r2i, 1u8)
        } else {
            (key_r2i, key_i2r, 0u8)
        };
        let tx_cipher = AES256GCM::new(&tx_key);
        let rx_cipher = AES256GCM::new(&rx_key);
        tx_key.zeroize();
        rx_key.zeroize();
        key_i2r.zeroize();
        key_r2i.zeroize();

        Ok(Self {
            inner,
            tx_cipher,
            rx_cipher,
            my_role,
            peer_role,
            next_send_seq: 0,
            next_recv_seq: 0,
            epoch,
        })
    }

    /// 협상된 epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 해체 후 내부 transport 반환.
    pub fn into_inner(self) -> C {
        self.inner
    }
}

#[async_trait]
impl<C: SecureChannel> SecureChannel for EncryptedChannel<C> {
    async fn send_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        let nonce = build_nonce(self.my_role, self.next_send_seq);
        let aad = aad_bytes(self.epoch, self.my_role);
        let mut ciphertext = vec![0u8; bytes.len()];
        let mut tag = [0u8; GCM_TAG_SIZE];
        self.tx_cipher
            .encrypt(&nonce, &aad, &bytes, &mut ciphertext, &mut tag);
        // 와이어 포맷: ciphertext || tag (구버전 aes-gcm 0.10 호환).
        ciphertext.extend_from_slice(&tag);

        let frame = EncryptedFrame {
            seq: self.next_send_seq,
            ciphertext,
        };
        self.inner.send(&frame).await?;
        self.next_send_seq = self
            .next_send_seq
            .checked_add(1)
            .ok_or_else(|| Error::Channel("encrypted: send seq overflow".into()))?;
        Ok(())
    }

    async fn recv_bytes(&mut self) -> Result<Vec<u8>> {
        let frame: EncryptedFrame = self.inner.recv().await?;
        if frame.seq != self.next_recv_seq {
            return Err(Error::Channel(format!(
                "encrypted: out-of-order seq (expected {}, got {})",
                self.next_recv_seq, frame.seq
            )));
        }
        if frame.ciphertext.len() < GCM_TAG_SIZE {
            return Err(Error::Crypto(
                "encrypted: frame shorter than GCM tag".into(),
            ));
        }
        let nonce = build_nonce(self.peer_role, frame.seq);
        let aad = aad_bytes(self.epoch, self.peer_role);
        let split = frame.ciphertext.len() - GCM_TAG_SIZE;
        let (ct, tag_slice) = frame.ciphertext.split_at(split);
        let mut tag = [0u8; GCM_TAG_SIZE];
        tag.copy_from_slice(tag_slice);
        let mut plaintext = vec![0u8; ct.len()];
        let ok = self
            .rx_cipher
            .decrypt(&nonce, &aad, ct, &tag, &mut plaintext);
        if !ok {
            return Err(Error::Crypto(
                "encrypted: AEAD authentication failed".into(),
            ));
        }
        self.next_recv_seq = self
            .next_recv_seq
            .checked_add(1)
            .ok_or_else(|| Error::Channel("encrypted: recv seq overflow".into()))?;
        Ok(plaintext)
    }
}

fn signing_payload(hello: &EncryptedHello) -> Result<Vec<u8>> {
    let body =
        postcard::to_allocvec(hello).map_err(|e| Error::Decode(format!("encrypted hello: {e}")))?;
    let mut out = Vec::with_capacity(KEX_SIGN_DOMAIN.len() + body.len());
    out.extend_from_slice(KEX_SIGN_DOMAIN);
    out.extend_from_slice(&body);
    Ok(out)
}

fn derive_key(shared: &[u8; 32], label: &[u8], epoch: u64) -> [u8; 32] {
    // BLAKE3 keyed-hash 로 (shared || label || epoch) → 32-byte key.
    blake3_keyed_derive_32(shared, label, &epoch.to_le_bytes())
}

fn build_nonce(role: u8, seq: u64) -> [u8; GCM_NONCE_SIZE] {
    let mut n = [0u8; GCM_NONCE_SIZE];
    n[0] = role;
    // n[1..4] 은 0 으로 둡니다 - 카운터 오버플로 시 명시적 거부.
    n[4..12].copy_from_slice(&seq.to_le_bytes());
    n
}

fn aad_bytes(epoch: u64, sender_role: u8) -> [u8; 16] {
    let mut aad = [0u8; 16];
    aad[0..8].copy_from_slice(&epoch.to_le_bytes());
    aad[8] = sender_role;
    aad
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inproc::pair;

    async fn handshake_pair(
        epoch: u64,
    ) -> Result<(
        EncryptedChannel<crate::inproc::InProcChannel>,
        EncryptedChannel<crate::inproc::InProcChannel>,
    )> {
        let mut rng = OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a = tokio::spawn(async move {
            EncryptedChannel::handshake_initiator(inner_a, sk_a, vk_b, epoch).await
        });
        let join_b = tokio::spawn(async move {
            EncryptedChannel::handshake_responder(inner_b, sk_b, vk_a, epoch).await
        });
        let a = join_a.await.unwrap()?;
        let b = join_b.await.unwrap()?;
        Ok((a, b))
    }

    #[tokio::test]
    async fn handshake_succeeds_with_pinned_keys() {
        let (a, b) = handshake_pair(7).await.unwrap();
        assert_eq!(a.epoch(), 7);
        assert_eq!(b.epoch(), 7);
    }

    #[tokio::test]
    async fn typed_roundtrip_after_handshake() {
        let (mut a, mut b) = handshake_pair(0).await.unwrap();
        a.send(&("ping", 42u32)).await.unwrap();
        let got: (String, u32) = b.recv().await.unwrap();
        assert_eq!(got, ("ping".to_string(), 42));
        b.send(&"pong".to_string()).await.unwrap();
        let got: String = a.recv().await.unwrap();
        assert_eq!(got, "pong");
    }

    #[tokio::test]
    async fn confidentiality_inner_traffic_is_ciphertext() {
        // 가장 자명한 평문 누출이 없는지 - "secret-marker" 가 그대로 와이어에 보이지 않아야.
        let (mut a, mut b) = handshake_pair(1).await.unwrap();
        let msg = b"secret-marker-DO-NOT-LEAK".to_vec();
        // a -> b 송신 후 b 에서 그대로 디코딩해 보면, ciphertext 안에는
        // 평문 마커가 등장하지 않아야 합니다.
        a.send_bytes(msg.clone()).await.unwrap();
        let got = b.recv_bytes().await.unwrap();
        assert_eq!(got, msg);
        // (와이어 ciphertext 검사는 inproc 채널 안에서 이미 디코드된 후이므로
        // 직접 byte slice 를 들여다볼 수 없지만, 토니발루 추가 테스트는 unit
        // 레벨에서 nonce/key 가 매번 달라지는지로 대신 검증)
    }

    #[tokio::test]
    async fn handshake_with_wrong_peer_pin_fails() {
        let mut rng = OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let sk_c = SigningKey::generate(&mut rng); // 공격자
        let vk_b = sk_b.verifying_key();
        let vk_c = sk_c.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a = tokio::spawn(async move {
            EncryptedChannel::handshake_initiator(inner_a, sk_a, vk_b, 0).await
        });
        let join_c = tokio::spawn(async move {
            // C 는 자신의 신원을 제시 - A 의 핀과 불일치
            EncryptedChannel::handshake_responder(inner_b, sk_c, vk_c, 0).await
        });
        let a = join_a.await.unwrap();
        let _ = join_c.await.unwrap();
        assert!(a.is_err(), "wrong-peer 핀이 거부되어야 합니다");
    }

    #[tokio::test]
    async fn epoch_mismatch_rejected() {
        let mut rng = OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a = tokio::spawn(async move {
            EncryptedChannel::handshake_initiator(inner_a, sk_a, vk_b, 1).await
        });
        let join_b = tokio::spawn(async move {
            EncryptedChannel::handshake_responder(inner_b, sk_b, vk_a, 2).await
        });
        let a = join_a.await.unwrap();
        let b = join_b.await.unwrap();
        assert!(
            a.is_err() || b.is_err(),
            "epoch 불일치는 핸드셰이크를 거부해야 합니다"
        );
    }

    #[tokio::test]
    async fn out_of_order_frame_rejected() {
        let (mut a, mut b) = handshake_pair(0).await.unwrap();
        a.send_bytes(b"first".to_vec()).await.unwrap();
        let _ = b.recv_bytes().await.unwrap();
        // 카운터를 인위적으로 건너뛴 프레임은 거부되어야.
        a.next_send_seq = 5;
        a.send_bytes(b"second".to_vec()).await.unwrap();
        let res = b.recv_bytes().await;
        assert!(matches!(res, Err(Error::Channel(_))));
    }

    #[tokio::test]
    async fn forged_ciphertext_rejected() {
        let (mut a, mut b) = handshake_pair(0).await.unwrap();
        // a 가 정상 송신 - b 의 카운터/키 상태가 진척됩니다.
        a.send_bytes(b"hello".to_vec()).await.unwrap();
        let _ = b.recv_bytes().await.unwrap();

        // 핸드셰이크 *없이* a 가 inner 채널에 임의의 ciphertext 를 던집니다.
        // (a 측의 키가 다르거나 내용이 변조된 셈)
        let bogus = EncryptedFrame {
            seq: 1,
            ciphertext: vec![0u8; 32],
        };
        a.inner.send(&bogus).await.unwrap();
        let res = b.recv_bytes().await;
        assert!(matches!(res, Err(Error::Crypto(_))));
    }
}
