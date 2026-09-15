//! Ed25519 래퍼 + serde 글루.
//!
//! `lumen-core` 가 Ed25519 구현 (`ed25519-dalek`) 에 직접 의존하는 유일한
//! 크레이트가 되어야 합니다. Capability 토큰과 provenance 서명 모두 이
//! 타입들을 거칩니다.
//!
//! 검증은 `verify_strict` 를 사용합니다. 소차수(small-order) 성분을 포함한
//! 공개키 / 서명과 비정규(non-canonical) 인코딩을 거부하여 서명 가단성
//! (malleability) 을 원천 차단합니다.

use std::fmt;

use ed25519_dalek::{
    Signature as DalekSignature, SignatureError, Signer, SigningKey as DalekSigningKey,
    VerifyingKey as DalekVerifyingKey,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};
use crate::rng::Rng;

/// Ed25519 비밀키 시드 길이 (32 바이트).
pub const SECRET_KEY_LENGTH: usize = 32;
/// Ed25519 서명 길이 (64 바이트).
pub const SIGNATURE_LENGTH: usize = 64;

/// Ed25519 서명 (개인) 키.
///
/// `ed25519_dalek::SigningKey` 를 감쌉니다. **절대 직렬화되지 않습니다** -
/// 개인 키 export 는 명시적이고 감사된 작업이어야 합니다. 그 이유로 래퍼는
/// 의도적으로 `Serialize` / `Display` impl 을 두지 않습니다. 내부 키는
/// drop 시 zeroize 됩니다.
#[derive(Clone)]
pub struct SigningKey {
    inner: DalekSigningKey,
}

impl SigningKey {
    /// CSPRNG 으로부터 새 키를 생성합니다.
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        rng.fill_bytes(&mut seed);
        let key = Self::from_seed(&seed);
        zeroize::Zeroize::zeroize(&mut seed);
        key
    }

    /// 32 바이트 원시 시드로부터 생성.
    pub fn from_seed(seed: &[u8; SECRET_KEY_LENGTH]) -> Self {
        Self {
            inner: DalekSigningKey::from_bytes(seed),
        }
    }

    /// 공개 검증 키를 도출합니다.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(self.inner.verifying_key())
    }

    /// 메시지에 서명합니다.
    pub fn sign(&self, msg: &[u8]) -> Signature {
        Signature(self.inner.sign(msg))
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey")
            .field("public", &self.verifying_key())
            .finish_non_exhaustive()
    }
}

/// Ed25519 검증 (공개) 키.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VerifyingKey(DalekVerifyingKey);

impl std::hash::Hash for VerifyingKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_bytes().hash(state);
    }
}

impl VerifyingKey {
    /// 32 바이트 원시 데이터로부터 파싱.
    ///
    /// # Errors
    /// 유효한 곡선 점으로 디코딩되지 않거나, 소차수(small-order) 성분을 가진
    /// 약한 키이면 [`Error::Crypto`] 를 반환합니다.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let key = DalekVerifyingKey::from_bytes(bytes).map_err(map_ed25519_err)?;
        if key.is_weak() {
            return Err(Error::Crypto(
                "signature: weak (small-order) public key rejected".into(),
            ));
        }
        Ok(Self(key))
    }

    /// 32 바이트 원시 데이터로 인코딩.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// 서명 검증 (strict).
    pub fn verify(&self, msg: &[u8], sig: &Signature) -> Result<()> {
        self.0.verify_strict(msg, &sig.0).map_err(map_ed25519_err)
    }

    /// 64자 소문자 hex 로 렌더.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.as_bytes())
    }
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerifyingKey({})", self.to_hex())
    }
}

impl fmt::Display for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for VerifyingKey {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        if ser.is_human_readable() {
            ser.serialize_str(&self.to_hex())
        } else {
            ser.serialize_bytes(self.0.as_bytes())
        }
    }
}

impl<'de> Deserialize<'de> for VerifyingKey {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        if de.is_human_readable() {
            let s = String::deserialize(de)?;
            let mut out = [0u8; 32];
            hex::decode_to_slice(s.trim(), &mut out).map_err(serde::de::Error::custom)?;
            Self::from_bytes(&out).map_err(serde::de::Error::custom)
        } else {
            let bytes = <Vec<u8>>::deserialize(de)?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| serde::de::Error::custom("verifying key must be 32 bytes"))?;
            Self::from_bytes(&arr).map_err(serde::de::Error::custom)
        }
    }
}

/// Ed25519 detached 서명.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature(DalekSignature);

impl Signature {
    /// 64 바이트 원시 데이터로부터 파싱.
    pub fn from_bytes(bytes: &[u8; SIGNATURE_LENGTH]) -> Self {
        Self(DalekSignature::from_bytes(bytes))
    }

    /// 64 바이트 원시 데이터로 렌더.
    pub fn to_bytes(&self) -> [u8; SIGNATURE_LENGTH] {
        self.0.to_bytes()
    }

    /// 소문자 hex (128자).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", self.to_hex())
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        if ser.is_human_readable() {
            ser.serialize_str(&self.to_hex())
        } else {
            ser.serialize_bytes(&self.0.to_bytes())
        }
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        if de.is_human_readable() {
            let s = String::deserialize(de)?;
            let mut out = [0u8; SIGNATURE_LENGTH];
            hex::decode_to_slice(s.trim(), &mut out).map_err(serde::de::Error::custom)?;
            Ok(Self::from_bytes(&out))
        } else {
            let bytes = <Vec<u8>>::deserialize(de)?;
            let arr: [u8; SIGNATURE_LENGTH] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| serde::de::Error::custom("signature must be 64 bytes"))?;
            Ok(Self::from_bytes(&arr))
        }
    }
}

fn map_ed25519_err(e: SignatureError) -> Error {
    Error::Crypto(format!("signature: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::OsRng;

    #[test]
    fn sign_verify_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let sig = sk.sign(b"hello");
        vk.verify(b"hello", &sig).unwrap();
        assert!(vk.verify(b"hellp", &sig).is_err());
    }

    #[test]
    fn vk_serde_human() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let json = serde_json::to_string(&vk).unwrap();
        let parsed: VerifyingKey = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, vk);
    }

    #[test]
    fn weak_public_key_rejected() {
        // 항등원 (소차수 점) 인코딩은 파싱 단계에서 거부되어야 합니다.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        assert!(VerifyingKey::from_bytes(&identity).is_err());
    }
}
