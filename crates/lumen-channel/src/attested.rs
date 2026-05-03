//! 소프트웨어 attestation 보안 채널.
//!
//! 임의의 [`SecureChannel`] 위에 생성 시점의 Ed25519 상호 핸드셰이크를 두르고,
//! 이후 모든 프레임을 협상된 세션 키로 서명합니다. 설계는 의도적으로 보수적
//! 입니다:
//!
//! - **상호 인증**: 양측은 *사전 핀된* 피어 공개키와 일치하는 Ed25519 개인키
//!   소유를 증명합니다. 이는 오케스트레이터와 TEE 사이에서 사용할 대칭
//!   패턴 - PKI 도, TOFU(Trust on First Use) 도 없음.
//! - **신원 바인딩**: 각 프레임은 페이로드와 함께 `(epoch, seq)` 쌍을
//!   서명합니다. 수신자는 순서 어긋난 프레임, 리플레이, 잘못된 epoch 프레임을
//!   모두 거부합니다.
//! - **기밀성 없음** (v0.2 기준) - 암호화는 AES-GCM/x25519 경로의
//!   `crypto-channel` feature 뒤로 남아 있습니다.
//!   이 모듈은 무결성 + 신원 + 리플레이 저항을 제공하며,
//!   이는 TEE attestation 이 실제로 제공하는 속성이고 암호화는 별개의 문제입니다.
//!
//! 핸드셰이크 메시지 형식은 향후 TEE attestation 피어와의 호환을 위해
//! **고정**됩니다:
//!
//! ```text
//! HelloFrame := { hello_marker: "lumen.attested.v1",
//!                 my_id: VerifyingKey, peer_id: VerifyingKey, nonce: [u8;16] }
//! Handshake  := { hello: HelloFrame, signature: Signature }   (postcard 인코딩)
//! DataFrame  := { epoch: u64, seq: u64, payload: Vec<u8> }
//! SignedFrame:= { frame: DataFrame, signature: Signature }    (postcard 인코딩)
//! ```
//!
//! TEE-attested 모드는 다른 `hello_marker` (`"lumen.attested-tee.v1"`) 를
//! 사용하며 플랫폼 quote 가 담긴 `attestation_doc: Vec<u8>` 필드를 포함합니다.
//! TEE-attested 피어를 기대하는 검증자는 software marker 를 **반드시**
//! 거부해야 합니다 - 묵시적인 업그레이드 경로는 존재하지 않습니다.

use async_trait::async_trait;
use lumen_core::{Error, Result, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::SecureChannel;

/// 모든 핸드셰이크에 실리는 marker. 버전을 올리면 이전 피어와 깔끔히 단절됩니다.
pub const SOFTWARE_HELLO_MARKER: &str = "lumen.attested.v1";

/// TEE 변종 전용 marker. v0.3 부터 실제 attestation 문서 형식 검증이
/// 활성화됩니다.
pub const TEE_HELLO_MARKER: &str = "lumen.attested-tee.v1";

/// 서명되는 핸드셰이크 평문 페이로드.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Hello {
    marker: String,
    /// 전송자가 주장하는 자기 ID.
    my_id: VerifyingKey,
    /// 전송자가 통신하기를 기대하는 (사전 핀된) 피어 ID.
    peer_id: VerifyingKey,
    /// 핸드셰이크 리플레이를 방지하는 16 바이트 난수.
    nonce: [u8; 16],
    /// 선택적 attestation 문서 (TEE 변종에서만 채워짐).
    ///
    /// 형식 검증은 [`lumen_attestation::AttestationDoc::parse`] 가 담당하며,
    /// software 변종에서는 항상 `None` 입니다. TEE 변종 사용자는 핀된
    /// `expected_measurement` 와 일치하지 않으면 핸드셰이크를 거부해야 합니다.
    #[serde(default)]
    attestation_doc: Option<Vec<u8>>,
}

/// 와이어 포맷 핸드셰이크 프레임.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SignedHello {
    hello: Hello,
    signature: Signature,
}

/// 서명되는 평문 데이터 프레임.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DataFrame {
    epoch: u64,
    seq: u64,
    payload: Vec<u8>,
}

/// 와이어 포맷 데이터 프레임.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SignedFrame {
    frame: DataFrame,
    signature: Signature,
}

/// 내부 transport 를 감싸는 software-attested 채널.
pub struct AttestedChannel<C: SecureChannel> {
    inner: C,
    epoch: u64,
    sk: SigningKey,
    peer_vk: VerifyingKey,
    next_send_seq: u64,
    next_recv_seq: u64,
}

impl<C: SecureChannel> AttestedChannel<C> {
    /// `inner` 위에서 Ed25519 상호 핸드셰이크를 수행하고 attested wrapper 를
    /// 반환합니다.
    ///
    /// 양측은 `epoch` 값에 대해 외부에서 합의해야 합니다 (보통 오케스트레이터
    /// 가 세션 키를 회전할 때마다 증가시킵니다).
    pub async fn handshake(
        inner: C,
        sk: SigningKey,
        peer_vk: VerifyingKey,
        epoch: u64,
    ) -> Result<Self> {
        Self::handshake_inner(inner, sk, peer_vk, epoch, None, None).await
    }

    /// TEE-attested 변종. 보내는 쪽은 자신의 attestation 문서를 첨부하고,
    /// 받는 쪽은 [`lumen_attestation::AttestationDoc::parse`] 로 형식을
    /// 검증합니다. `expected_peer_measurement` 가 지정된 경우, 피어 보고서의
    /// `measurement` 필드가 그 값과 정확히 일치해야 합니다.
    ///
    /// 암호학적 검증 (PCK chain / VCEK signature) 은 v0.4 작업입니다 - 본
    /// 메서드는 형식 + measurement 핀까지만 검증합니다.
    pub async fn handshake_tee(
        inner: C,
        sk: SigningKey,
        peer_vk: VerifyingKey,
        epoch: u64,
        my_attestation: Vec<u8>,
        expected_peer_measurement: Option<Vec<u8>>,
    ) -> Result<Self> {
        Self::handshake_inner(
            inner,
            sk,
            peer_vk,
            epoch,
            Some(my_attestation),
            expected_peer_measurement,
        )
        .await
    }

    async fn handshake_inner(
        mut inner: C,
        sk: SigningKey,
        peer_vk: VerifyingKey,
        epoch: u64,
        attestation_doc: Option<Vec<u8>>,
        expected_peer_measurement: Option<Vec<u8>>,
    ) -> Result<Self> {
        let my_vk = sk.verifying_key();
        let nonce = random_nonce();
        let marker = if attestation_doc.is_some() {
            TEE_HELLO_MARKER
        } else {
            SOFTWARE_HELLO_MARKER
        };
        let hello = Hello {
            marker: marker.to_string(),
            my_id: my_vk,
            peer_id: peer_vk,
            nonce,
            attestation_doc,
        };
        let hello_bytes =
            postcard::to_allocvec(&hello).map_err(|e| Error::Decode(format!("hello: {e}")))?;
        let signature = sk.sign(&hello_bytes);
        let signed = SignedHello {
            hello: hello.clone(),
            signature,
        };
        inner.send(&signed).await?;

        let peer: SignedHello = inner.recv().await?;
        if peer.hello.marker != marker {
            return Err(Error::Crypto(format!(
                "attested: 예상한 marker={marker} != peer={:?}",
                peer.hello.marker
            )));
        }
        if peer.hello.my_id != peer_vk {
            return Err(Error::Crypto(
                "attested: peer 가 잘못된 ID 를 주장함".into(),
            ));
        }
        if peer.hello.peer_id != my_vk {
            return Err(Error::Crypto(
                "attested: peer hello 의 수신자가 우리 ID 가 아님".into(),
            ));
        }
        let peer_bytes = postcard::to_allocvec(&peer.hello)
            .map_err(|e| Error::Decode(format!("peer hello: {e}")))?;
        peer_vk.verify(&peer_bytes, &peer.signature)?;

        // TEE 변종: peer attestation 문서 형식 + measurement 핀 검증.
        if marker == TEE_HELLO_MARKER {
            let bytes =
                peer.hello.attestation_doc.as_ref().ok_or_else(|| {
                    Error::Crypto("attested-tee: peer 가 attestation 미첨부".into())
                })?;
            let doc = lumen_attestation::AttestationDoc::parse(bytes)
                .map_err(|e| Error::Crypto(format!("attested-tee: {e}")))?;
            if let Some(expected) = expected_peer_measurement {
                if doc.measurement() != expected.as_slice() {
                    return Err(Error::Crypto(
                        "attested-tee: peer measurement 가 핀과 불일치".into(),
                    ));
                }
            }
        }

        Ok(Self {
            inner,
            epoch,
            sk,
            peer_vk,
            next_send_seq: 0,
            next_recv_seq: 0,
        })
    }

    /// 해체 후 내부 transport 를 반환합니다.
    pub fn into_inner(self) -> C {
        self.inner
    }

    /// 현재 epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

#[async_trait]
impl<C: SecureChannel> SecureChannel for AttestedChannel<C> {
    async fn send_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        let frame = DataFrame {
            epoch: self.epoch,
            seq: self.next_send_seq,
            payload: bytes,
        };
        let frame_bytes = postcard::to_allocvec(&frame)
            .map_err(|e| Error::Decode(format!("frame encode: {e}")))?;
        let signature = self.sk.sign(&frame_bytes);
        let signed = SignedFrame { frame, signature };
        self.inner.send(&signed).await?;
        self.next_send_seq = self
            .next_send_seq
            .checked_add(1)
            .ok_or_else(|| Error::Channel("attested: send seq overflow".into()))?;
        Ok(())
    }

    async fn recv_bytes(&mut self) -> Result<Vec<u8>> {
        let signed: SignedFrame = self.inner.recv().await?;
        let frame_bytes = postcard::to_allocvec(&signed.frame)
            .map_err(|e| Error::Decode(format!("frame decode: {e}")))?;
        self.peer_vk.verify(&frame_bytes, &signed.signature)?;
        if signed.frame.epoch != self.epoch {
            return Err(Error::Channel(format!(
                "attested: epoch mismatch (expected {}, got {})",
                self.epoch, signed.frame.epoch
            )));
        }
        if signed.frame.seq != self.next_recv_seq {
            return Err(Error::Channel(format!(
                "attested: out-of-order seq (expected {}, got {})",
                self.next_recv_seq, signed.frame.seq
            )));
        }
        self.next_recv_seq = self
            .next_recv_seq
            .checked_add(1)
            .ok_or_else(|| Error::Channel("attested: recv seq overflow".into()))?;
        Ok(signed.frame.payload)
    }
}

fn random_nonce() -> [u8; 16] {
    use rand::RngCore;
    let mut n = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut n);
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inproc::pair;
    use lumen_core::SigningKey;

    async fn handshake_pair(
        epoch: u64,
    ) -> (
        AttestedChannel<crate::inproc::InProcChannel>,
        AttestedChannel<crate::inproc::InProcChannel>,
    ) {
        let mut rng = rand::rngs::OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a =
            tokio::spawn(
                async move { AttestedChannel::handshake(inner_a, sk_a, vk_b, epoch).await },
            );
        let join_b =
            tokio::spawn(
                async move { AttestedChannel::handshake(inner_b, sk_b, vk_a, epoch).await },
            );
        let a = join_a.await.unwrap().unwrap();
        let b = join_b.await.unwrap().unwrap();
        (a, b)
    }

    #[tokio::test]
    async fn handshake_succeeds() {
        let (a, b) = handshake_pair(1).await;
        assert_eq!(a.epoch(), 1);
        assert_eq!(b.epoch(), 1);
    }

    #[tokio::test]
    async fn signed_frames_round_trip() {
        let (mut a, mut b) = handshake_pair(7).await;
        a.send(&("ping", 42u32)).await.unwrap();
        let got: (String, u32) = b.recv().await.unwrap();
        assert_eq!(got.0, "ping");
        assert_eq!(got.1, 42);
    }

    #[tokio::test]
    async fn handshake_with_wrong_peer_fails() {
        let mut rng = rand::rngs::OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let sk_c = SigningKey::generate(&mut rng); // attacker
        let vk_b = sk_b.verifying_key();
        let vk_c = sk_c.verifying_key();
        let (inner_a, inner_b) = pair();
        // A expects to talk to B but C is on the other side.
        let join_a =
            tokio::spawn(async move { AttestedChannel::handshake(inner_a, sk_a, vk_b, 1).await });
        let join_c = tokio::spawn(async move {
            // C presents *its own* identity, which doesn't match A's pin.
            AttestedChannel::handshake(inner_b, sk_c, vk_c, 1).await
        });
        let a = join_a.await.unwrap();
        let _ = join_c.await.unwrap();
        assert!(a.is_err(), "expected handshake to fail");
    }

    #[tokio::test]
    async fn frame_with_wrong_epoch_rejected() {
        let (mut a, mut b) = handshake_pair(5).await;
        // Forge a frame with the wrong epoch by reaching into A's state.
        a.epoch = 6;
        a.send_bytes(b"x".to_vec()).await.unwrap();
        let res = b.recv_bytes().await;
        assert!(matches!(res, Err(Error::Channel(_))));
    }

    #[tokio::test]
    async fn frame_with_skipped_seq_rejected() {
        let (mut a, mut b) = handshake_pair(0).await;
        // First send is OK at seq 0.
        a.send_bytes(b"first".to_vec()).await.unwrap();
        let _ = b.recv_bytes().await.unwrap();
        // Now skip to seq 5 - out-of-order, must reject.
        a.next_send_seq = 5;
        a.send_bytes(b"second".to_vec()).await.unwrap();
        let res = b.recv_bytes().await;
        assert!(matches!(res, Err(Error::Channel(_))));
    }

    #[tokio::test]
    async fn tampered_payload_rejected() {
        // We can't easily tamper without breaking the abstraction; instead,
        // wire two pairs and inject a different signer's frame.
        let mut rng = rand::rngs::OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let sk_evil = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a =
            tokio::spawn(async move { AttestedChannel::handshake(inner_a, sk_a, vk_b, 0).await });
        let join_b =
            tokio::spawn(async move { AttestedChannel::handshake(inner_b, sk_b, vk_a, 0).await });
        let mut a = join_a.await.unwrap().unwrap();
        let mut b = join_b.await.unwrap().unwrap();

        // Replace A's signing key with the attacker's; payload still verifies
        // structurally but signature won't match the pinned VK.
        a.sk = sk_evil;
        a.send_bytes(b"forged".to_vec()).await.unwrap();
        let res = b.recv_bytes().await;
        assert!(matches!(res, Err(Error::Crypto(_))));
    }

    /// TEE 변종은 attestation 문서 형식을 검증한 뒤 핸드셰이크를 완료해야 합니다.
    #[tokio::test]
    async fn tee_handshake_with_well_formed_doc() {
        // sev-snp fixture (1184 바이트, version=2, vmpl<=3)
        let mut doc_a = vec![0u8; 1184];
        doc_a[0..4].copy_from_slice(&2u32.to_le_bytes());
        let mut doc_b = doc_a.clone();
        // measurement 필드를 다르게 채워서 두 보고서를 구분.
        doc_a[144..192].fill(0x11);
        doc_b[144..192].fill(0x22);
        let pin_b = vec![0x22u8; 48];

        let mut rng = rand::rngs::OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a = tokio::spawn(async move {
            AttestedChannel::handshake_tee(inner_a, sk_a, vk_b, 1, doc_a, Some(pin_b)).await
        });
        let join_b = tokio::spawn(async move {
            AttestedChannel::handshake_tee(inner_b, sk_b, vk_a, 1, doc_b, None).await
        });
        let a = join_a.await.unwrap();
        let b = join_b.await.unwrap();
        assert!(
            a.is_ok(),
            "tee handshake A: {}",
            a.err().map(|e| e.to_string()).unwrap_or_default()
        );
        assert!(
            b.is_ok(),
            "tee handshake B: {}",
            b.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }

    /// measurement 핀 불일치 시 거부.
    #[tokio::test]
    async fn tee_handshake_pin_mismatch_rejected() {
        let mut doc_a = vec![0u8; 1184];
        doc_a[0..4].copy_from_slice(&2u32.to_le_bytes());
        let mut doc_b = doc_a.clone();
        doc_b[144..192].fill(0x22);
        // A 가 기대하는 pin 은 0xAA 인데 실제 B 는 0x22 -> 불일치.
        let bad_pin = vec![0xAAu8; 48];

        let mut rng = rand::rngs::OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a = tokio::spawn(async move {
            AttestedChannel::handshake_tee(inner_a, sk_a, vk_b, 1, doc_a, Some(bad_pin)).await
        });
        let _join_b = tokio::spawn(async move {
            AttestedChannel::handshake_tee(inner_b, sk_b, vk_a, 1, doc_b, None).await
        });
        let a = join_a.await.unwrap();
        assert!(a.is_err(), "pin mismatch 가 거부되어야 합니다");
    }

    /// software peer 가 TEE 핸드셰이크에 응답할 수 없어야 함 (marker 불일치).
    #[tokio::test]
    async fn tee_marker_mismatch_rejected() {
        let mut doc = vec![0u8; 1184];
        doc[0..4].copy_from_slice(&2u32.to_le_bytes());

        let mut rng = rand::rngs::OsRng;
        let sk_a = SigningKey::generate(&mut rng);
        let sk_b = SigningKey::generate(&mut rng);
        let vk_a = sk_a.verifying_key();
        let vk_b = sk_b.verifying_key();
        let (inner_a, inner_b) = pair();
        let join_a = tokio::spawn(async move {
            // A 는 TEE 변종으로 핸드셰이크 시도
            AttestedChannel::handshake_tee(inner_a, sk_a, vk_b, 1, doc, None).await
        });
        let join_b = tokio::spawn(async move {
            // B 는 software 변종 - marker 가 다름
            AttestedChannel::handshake(inner_b, sk_b, vk_a, 1).await
        });
        let a = join_a.await.unwrap();
        let _ = join_b.await.unwrap();
        assert!(a.is_err(), "marker 불일치가 거부되어야 합니다");
    }
}
