//! 암호 프리미티브 KAT (Known-Answer Test) 회귀 테스트.
//!
//! Lumen 의 래퍼 (`lumen-core`) 와 `lumen-channel` 이 사용하는 원시
//! 프리미티브가 공개 표준 벡터와 **비트-동일한 출력** 을 산출함을 보증합니다.
//! 이 테스트가 통과하면 Lumen 은 표준 준수 구현체 (폐쇄망 K0 피어 포함) 와
//! 와이어 호환됩니다.
//!
//! # 검증 범위
//! - BLAKE3: 공식 test_vectors.json (hash / keyed_hash / derive_key)
//! - Ed25519: RFC 8032 §7.1 TEST 1 / TEST 2
//! - X25519: RFC 7748 §6.1 Alice / Bob 키 합의
//! - AES-256-GCM: NIST GCM 사양 Test Case 16
//! - 결정성: 동일 입력 -> 동일 출력 (RNG 가 끼지 않은 경로)

use lumen_core::hash::{blake3_keyed_derive_32, Blake3Hash};
use lumen_core::{Signature, SigningKey, VerifyingKey};

fn hex32(s: &str) -> [u8; 32] {
    let v = hex::decode(s).expect("hex");
    v.as_slice().try_into().expect("32 bytes")
}

fn hex64(s: &str) -> [u8; 64] {
    let v = hex::decode(s).expect("hex");
    v.as_slice().try_into().expect("64 bytes")
}

/// BLAKE3 공식 벡터의 입력 패턴: 251 주기의 0..=250 바이트 반복.
fn blake3_pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

const BLAKE3_KEY: &[u8; 32] = b"whats the Elvish word for friend";
const BLAKE3_CONTEXT: &str = "BLAKE3 2019-12-27 16:29:52 test vectors context";

#[test]
fn blake3_hash_matches_official_vectors() {
    let cases: &[(usize, &str)] = &[
        (
            0,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        ),
        (
            1,
            "2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213",
        ),
        (
            1024,
            "42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7",
        ),
        (
            1025,
            "d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444",
        ),
    ];
    for (len, expected) in cases {
        let input = blake3_pattern(*len);
        assert_eq!(
            Blake3Hash::of(&input).as_bytes(),
            &hex32(expected),
            "BLAKE3 hash mismatch (input len={len})"
        );
    }
}

#[test]
fn blake3_hash_of_file_matches_official_vector() {
    let dir = std::env::temp_dir().join(format!("lumen-kat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("blake3-1025.bin");
    std::fs::write(&path, blake3_pattern(1025)).unwrap();
    let hashed = Blake3Hash::of_file(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        hashed.as_bytes(),
        &hex32("d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444")
    );
}

#[test]
fn blake3_keyed_derive_matches_official_keyed_vector() {
    // `blake3_keyed_derive_32(key, label, extra)` 는 keyed_hash(key, label || extra)
    // 와 동일해야 하며, 공식 keyed_hash 벡터로 고정합니다.
    let cases: &[(usize, &str)] = &[
        (
            0,
            "92b2b75604ed3c761f9d6f62392c8a9227ad0ea3f09573e783f1498a4ed60d26",
        ),
        (
            1024,
            "75c46f6f3d9eb4f55ecaaee480db732e6c2105546f1e675003687c31719c7ba4",
        ),
        (
            1025,
            "357dc55de0c7e382c900fd6e320acc04146be01db6a8ce7210b7189bd664ea69",
        ),
    ];
    for (len, expected) in cases {
        let input = blake3_pattern(*len);
        let (label, extra) = input.split_at(len / 2);
        let derived = blake3_keyed_derive_32(BLAKE3_KEY, label, extra);
        assert_eq!(
            derived,
            hex32(expected),
            "BLAKE3 keyed_hash mismatch (len={len})"
        );
    }
}

#[test]
fn blake3_derive_key_matches_official_vector() {
    // `lumen_core::rng` 의 DRBG 시드 도출이 의존하는 derive_key 모드 회귀 고정.
    let cases: &[(usize, &str)] = &[
        (
            0,
            "2cc39783c223154fea8dfb7c1b1660f2ac2dcbd1c1de8277b0b0dd39b7e50d7d",
        ),
        (
            1024,
            "7356cd7720d5b66b6d0697eb3177d9f8d73a4a5c5e968896eb6a689684302706",
        ),
        (
            1025,
            "effaa245f065fbf82ac186839a249707c3bddf6d3fdda22d1b95a3c970379bcb",
        ),
    ];
    for (len, expected) in cases {
        let out = blake3::derive_key(BLAKE3_CONTEXT, &blake3_pattern(*len));
        assert_eq!(
            out,
            hex32(expected),
            "BLAKE3 derive_key mismatch (len={len})"
        );
    }
}

#[test]
fn ed25519_rfc8032_test_vectors() {
    // (secret seed, public key, message, signature)
    let cases: &[(&str, &str, &[u8], &str)] = &[
        (
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            b"",
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        ),
        (
            "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            &[0x72],
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        ),
    ];
    for (seed, pk, msg, sig) in cases {
        let sk = SigningKey::from_seed(&hex32(seed));
        let vk = sk.verifying_key();
        assert_eq!(vk.to_bytes(), hex32(pk), "RFC 8032 public key mismatch");
        let signature = sk.sign(msg);
        assert_eq!(
            signature.to_bytes(),
            hex64(sig),
            "RFC 8032 signature mismatch"
        );
        vk.verify(msg, &signature)
            .expect("RFC 8032 signature must verify");

        let parsed_vk = VerifyingKey::from_bytes(&hex32(pk)).unwrap();
        let parsed_sig = Signature::from_bytes(&hex64(sig));
        parsed_vk
            .verify(msg, &parsed_sig)
            .expect("wire-parsed key/signature must verify");
    }
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
fn ed25519_tampered_signature_rejected() {
    let sk = SigningKey::from_seed(&[0x11u8; 32]);
    let vk = sk.verifying_key();
    let mut raw = sk.sign(b"payload").to_bytes();
    raw[0] ^= 0x01;
    assert!(vk.verify(b"payload", &Signature::from_bytes(&raw)).is_err());
}

#[test]
fn x25519_rfc7748_key_agreement_vector() {
    use x25519_dalek::{PublicKey, StaticSecret};

    let alice_sk = StaticSecret::from(hex32(
        "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a",
    ));
    let bob_sk = StaticSecret::from(hex32(
        "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb",
    ));
    let alice_pk = PublicKey::from(&alice_sk);
    let bob_pk = PublicKey::from(&bob_sk);
    assert_eq!(
        alice_pk.to_bytes(),
        hex32("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
    );
    assert_eq!(
        bob_pk.to_bytes(),
        hex32("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
    );

    let expected = hex32("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
    let shared_a = alice_sk.diffie_hellman(&bob_pk);
    let shared_b = bob_sk.diffie_hellman(&alice_pk);
    assert!(shared_a.was_contributory());
    assert_eq!(shared_a.as_bytes(), &expected);
    assert_eq!(shared_b.as_bytes(), &expected);

    // 와이어로 운반된 32 바이트 공개키만으로 라운드트립 가능해야 함.
    let bob_pk_wire = PublicKey::from(bob_pk.to_bytes());
    assert_eq!(alice_sk.diffie_hellman(&bob_pk_wire).as_bytes(), &expected);
}

#[test]
fn x25519_small_order_point_is_non_contributory() {
    use x25519_dalek::{PublicKey, StaticSecret};

    let sk = StaticSecret::from([0x42u8; 32]);
    let identity = PublicKey::from([0u8; 32]);
    let shared = sk.diffie_hellman(&identity);
    assert!(
        !shared.was_contributory(),
        "identity point must be flagged non-contributory"
    );
    assert_eq!(shared.as_bytes(), &[0u8; 32]);
}

#[test]
fn aes256_gcm_nist_test_case_16() {
    use aes_gcm::aead::{AeadInOut, Nonce, Tag};
    use aes_gcm::{Aes256Gcm, KeyInit};

    let key = hex32("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
    let iv: [u8; 12] = hex::decode("cafebabefacedbaddecaf888")
        .unwrap()
        .try_into()
        .unwrap();
    let aad = hex::decode("feedfacedeadbeeffeedfacedeadbeefabaddad2").unwrap();
    let plaintext = hex::decode(
        "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
    )
    .unwrap();
    let expected_ct = hex::decode(
        "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
    )
    .unwrap();
    let expected_tag: [u8; 16] = hex::decode("76fc6ece0f4e1768cddf8853bb2d551b")
        .unwrap()
        .try_into()
        .unwrap();

    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::<Aes256Gcm>::from(iv);

    let mut buf = plaintext.clone();
    let tag = cipher
        .encrypt_inout_detached(&nonce, &aad, buf.as_mut_slice().into())
        .expect("encrypt");
    assert_eq!(buf, expected_ct, "NIST GCM TC16 ciphertext mismatch");
    assert_eq!(tag.as_slice(), &expected_tag, "NIST GCM TC16 tag mismatch");

    // lumen-channel 와이어 포맷 (ciphertext || tag) 라운드트립.
    let mut wire = buf.clone();
    wire.extend_from_slice(&tag);
    let tag_back = wire.split_off(wire.len() - 16);
    let tag_back = Tag::<Aes256Gcm>::from(<[u8; 16]>::try_from(tag_back.as_slice()).unwrap());
    cipher
        .decrypt_inout_detached(&nonce, &aad, wire.as_mut_slice().into(), &tag_back)
        .expect("decrypt");
    assert_eq!(wire, plaintext);

    // 태그 1 비트 변조는 거부되어야 함.
    let mut bad_tag = expected_tag;
    bad_tag[0] ^= 0x80;
    let mut tampered = expected_ct.clone();
    assert!(cipher
        .decrypt_inout_detached(
            &nonce,
            &aad,
            tampered.as_mut_slice().into(),
            &Tag::<Aes256Gcm>::from(bad_tag),
        )
        .is_err());
}
