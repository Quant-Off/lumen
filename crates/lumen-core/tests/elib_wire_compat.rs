//! 폐쇄 커널(K0) 환경 호환성 회귀 테스트.
//!
//! Lumen 의 `lumen-core` 래퍼가 `elib-k0-nt` 의 모듈을 *직접* 호출했을 때와
//! **비트-동일한 출력** 을 산출함을 보증합니다. K0 의 elib-k0d-core
//! 디스패처는 elib-k0-nt 를 직접 호출하므로, 본 테스트가 통과하면 lumen 이
//! 폐쇄망 피어와 와이어 호환됩니다.
//!
//! # 검증 범위
//! - BLAKE3: `Blake3Hash::of` ≡ 직접 호출
//! - BLAKE3 keyed KDF: `blake3_keyed_derive_32` ≡ 직접 호출
//! - Ed25519: `SigningKey::sign` ≡ `elib_ed25519::sign`
//! - Ed25519: `VerifyingKey::verify` ≡ `elib_ed25519::verify`
//! - x25519 ECDH: 양방향 공유 비밀 일치
//! - 결정성: 동일 입력 → 동일 출력 (RNG 가 끼지 않은 경로)

use elib_blake::Blake3;
use elib_ed25519::{
    self as ed25519, PublicKey as RawPublicKey, SecretKey as RawSecretKey,
    Signature as RawSignature,
};
use lumen_core::hash::{blake3_keyed_derive_32, Blake3Hash};
use lumen_core::{SigningKey, VerifyingKey};

fn collect_finalize(input: &[u8]) -> [u8; 32] {
    let mut h = Blake3::new();
    h.update(input);
    let buf = h.finalize().expect("finalize");
    let mut out = [0u8; 32];
    out.copy_from_slice(buf.as_slice());
    out
}

#[test]
fn blake3_wrapper_byte_identical_to_direct_elib() {
    let inputs: &[&[u8]] = &[b"", b"a", b"hello world", b"lumen.air-gap.kernel.compat.v1"];
    for input in inputs {
        let lumen = Blake3Hash::of(input);
        let direct = collect_finalize(input);
        assert_eq!(
            lumen.as_bytes(),
            &direct,
            "BLAKE3 wrapper must match direct elib-blake call (input len={})",
            input.len()
        );
    }
}

#[test]
fn blake3_wrapper_byte_identical_for_large_input() {
    // 청크 경계(>1 KiB) 와 cv-스택 머지가 모두 일어나는 크기.
    let big = vec![0xA5u8; 16 * 1024];
    let lumen = Blake3Hash::of(&big);
    let direct = collect_finalize(&big);
    assert_eq!(lumen.as_bytes(), &direct);
}

#[test]
fn blake3_keyed_derive_byte_identical_to_direct_elib() {
    // lumen-channel 의 KDF 가 폐쇄망 피어의 KDF 와 비트-동일함을 회귀 보장.
    let key = [0x11u8; 32];
    let label = b"lumen.encrypted.k-i2r.v1";
    let epoch_le = 42u64.to_le_bytes();

    let via_wrapper = blake3_keyed_derive_32(&key, label, &epoch_le);

    let mut h = Blake3::new_keyed(&key);
    h.update(label);
    h.update(&epoch_le);
    let buf = h.finalize().expect("finalize");
    let mut direct = [0u8; 32];
    direct.copy_from_slice(buf.as_slice());

    assert_eq!(via_wrapper, direct);
}

#[test]
fn ed25519_sign_verify_via_wrapper_matches_direct_elib() {
    // 와이어로 운반되는 객체는 (msg, signature, public_key) 3-튜플이므로
    // - lumen 으로 서명하고 elib 직접 검증
    // - elib 로 서명하고 lumen 직접 검증
    // 두 방향 모두에서 동일하게 통과해야 합니다.
    let seed = [0x42u8; 32];
    let msg = b"lumen.kernel.compat.ed25519.v1";

    // lumen 측 서명 → elib 측 검증
    let sk_lumen = SigningKey::from_seed(&seed);
    let vk_lumen = sk_lumen.verifying_key();
    let sig_lumen = sk_lumen.sign(msg);

    let raw_sk = RawSecretKey::from_bytes(&seed);
    let raw_pk = RawPublicKey::from(&raw_sk);
    assert_eq!(*raw_pk.as_bytes(), vk_lumen.to_bytes());

    let raw_sig = RawSignature::from_bytes(&sig_lumen.to_bytes());
    ed25519::verify(msg, &raw_sig, &raw_pk).expect("elib-direct verify of lumen signature");

    // elib 측 서명 → lumen 측 검증
    let direct_sig = ed25519::sign(msg, &raw_sk);
    assert_eq!(
        direct_sig.as_bytes(),
        &sig_lumen.to_bytes(),
        "Ed25519 결정론적 서명: lumen 과 elib 결과가 비트-동일해야 함"
    );

    let vk_lumen_via_raw = VerifyingKey::from_bytes(raw_pk.as_bytes()).unwrap();
    let lumen_sig_obj = lumen_core::Signature::from_bytes(direct_sig.as_bytes());
    vk_lumen_via_raw
        .verify(msg, &lumen_sig_obj)
        .expect("lumen-wrapper verify of elib direct signature");
}

#[test]
fn ed25519_signature_is_deterministic() {
    // RFC 8032 Ed25519 는 결정론적 서명입니다 - 동일 (msg, sk) 는 항상 동일
    // 서명을 산출해야 하며, 와이어 호환성 회귀의 사전 조건입니다.
    let seed = [0x77u8; 32];
    let sk = SigningKey::from_seed(&seed);
    let s1 = sk.sign(b"msg-1");
    let s2 = sk.sign(b"msg-1");
    assert_eq!(s1.to_bytes(), s2.to_bytes());
}

#[test]
fn x25519_ecdh_produces_matching_shared_secret() {
    // 양 피어가 elib_x25519 만으로 공유 비밀을 도출할 때 비트-동일한 32 바이트
    // 결과를 얻는다는 회귀 보장. lumen-channel::EncryptedChannel 의 핸드셰이크가
    // 이 동작 위에 의존합니다.
    use elib_x25519::{PublicKey, SecretKey};

    let sk_a = SecretKey::from_bytes([0x01u8; 32]);
    let sk_b = SecretKey::from_bytes([0x02u8; 32]);
    let pk_a = sk_a.public_key();
    let pk_b = sk_b.public_key();

    // 한 쪽이 만든 공유 비밀은 다른 쪽이 자신의 secret 으로 만든 결과와 동일.
    let shared_a = sk_a.diffie_hellman(&pk_b);
    let shared_b = sk_b.diffie_hellman(&pk_a);
    assert_eq!(shared_a.as_bytes(), shared_b.as_bytes());

    // 와이어로 운반된 32 바이트 공개키만으로 라운드트립 가능해야 함.
    let pk_a_wire: [u8; 32] = *pk_a.as_bytes();
    let pk_b_wire: [u8; 32] = *pk_b.as_bytes();
    let shared_a_again = sk_a.diffie_hellman(&PublicKey::from_bytes(pk_b_wire));
    let shared_b_again = sk_b.diffie_hellman(&PublicKey::from_bytes(pk_a_wire));
    assert_eq!(shared_a_again.as_bytes(), shared_b_again.as_bytes());
    assert_eq!(shared_a_again.as_bytes(), shared_a.as_bytes());
}

#[test]
fn aes256_gcm_encrypts_with_tag_appended_format_used_by_lumen_channel() {
    // lumen-channel 은 와이어 포맷을 `ciphertext || tag` 로 직렬화합니다.
    // elib-k0-nt 의 raw API 는 (ct, tag) 분리 출력이므로 두 형태가 정확히
    // 동일한 17~N 바이트 와이어를 만드는지 회귀 보장.
    use elib_aes::{AES256GCM, GCM_NONCE_SIZE, GCM_TAG_SIZE};

    let key = [0x33u8; 32];
    let nonce = [0x44u8; GCM_NONCE_SIZE];
    let aad = b"lumen.aad.v1";
    let plaintext = b"lumen.kernel.compat.aesgcm.v1";

    let cipher = AES256GCM::new(&key);
    let mut ct_a = vec![0u8; plaintext.len()];
    let mut tag_a = [0u8; GCM_TAG_SIZE];
    cipher.encrypt(&nonce, aad, plaintext, &mut ct_a, &mut tag_a);

    let mut ct_b = vec![0u8; plaintext.len()];
    let mut tag_b = [0u8; GCM_TAG_SIZE];
    cipher.encrypt(&nonce, aad, plaintext, &mut ct_b, &mut tag_b);

    // 결정성: 동일 (key, nonce, aad, pt) → 동일 (ct, tag)
    assert_eq!(ct_a, ct_b);
    assert_eq!(tag_a, tag_b);

    // 와이어 형식: ciphertext || tag
    let mut wire = ct_a.clone();
    wire.extend_from_slice(&tag_a);

    // 디코드 측 검증 (lumen-channel 이 수행하는 절차와 동일)
    let split = wire.len() - GCM_TAG_SIZE;
    let (ct_back, tag_back) = wire.split_at(split);
    let mut tag_arr = [0u8; GCM_TAG_SIZE];
    tag_arr.copy_from_slice(tag_back);
    let mut pt_back = vec![0u8; ct_back.len()];
    let ok = cipher.decrypt(&nonce, aad, ct_back, &tag_arr, &mut pt_back);
    assert!(ok, "AEAD 검증 실패");
    assert_eq!(pt_back, plaintext);
}
