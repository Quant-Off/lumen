//! Ed25519 래퍼 + serde 글루.
//!
//! `lumen-core` 가 Ed25519 구현 (`elib-k0-nt/ed25519`) 에 직접 의존하는
//! 유일한 크레이트가 되어야 합니다. Capability 토큰과 provenance 서명
//! 모두 이 타입들을 거칩니다.
//!
//! 폐쇄형(Air-Gapped) iso-light-k0 환경 호환을 위해 외부 `ed25519-dalek`
//! 크레이트는 모두 elib-k0-nt 모듈로 교체되었습니다.

use std::fmt;

use elib_ed25519::{self as ed25519, Ed25519Error};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};
use crate::rng::Rng;

/// Ed25519 비밀키 시드 길이 (32 바이트).
pub const SECRET_KEY_LENGTH: usize = 32;
/// Ed25519 서명 길이 (64 바이트).
pub const SIGNATURE_LENGTH: usize = 64;

/// Ed25519 서명 (개인) 키.
///
/// `elib-k0-nt/ed25519::SecretKey` 를 감쌉니다. **절대 직렬화되지 않습니다** -
/// 개인 키 export 는 명시적이고 감사된 작업이어야 합니다. 그 이유로 래퍼는
/// 의도적으로 `Serialize` / `Display` impl 을 두지 않습니다.
#[derive(Clone)]
pub struct SigningKey {
    secret: ed25519::SecretKey,
    /// 공개 검증 키. 시드에서 매번 재유도하지 않도록 캐시.
    public: ed25519::PublicKey,
}

impl SigningKey {
    /// CSPRNG 으로부터 새 키를 생성합니다.
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        rng.fill_bytes(&mut seed);
        Self::from_seed(&seed)
    }

    /// 32 바이트 원시 시드로부터 생성.
    pub fn from_seed(seed: &[u8; SECRET_KEY_LENGTH]) -> Self {
        let secret = ed25519::SecretKey::from_bytes(seed);
        let public = ed25519::PublicKey::from(&secret);
        Self { secret, public }
    }

    /// 공개 검증 키를 도출합니다.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(self.public)
    }

    /// 메시지에 서명합니다.
    pub fn sign(&self, msg: &[u8]) -> Signature {
        Signature(ed25519::sign(msg, &self.secret))
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
pub struct VerifyingKey(ed25519::PublicKey);

impl std::hash::Hash for VerifyingKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_bytes().hash(state);
    }
}

impl VerifyingKey {
    /// 32 바이트 원시 데이터로부터 파싱.
    ///
    /// 현재 elib-k0-nt 의 `PublicKey::from_bytes` 는 형태 검증 없이 32 바이트를
    /// 그대로 받아들이고, 실제 곡선 점 디코딩은 `verify` 시점에 수행됩니다.
    /// API 계약 유지를 위해 길이 검증만 수행합니다.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        Ok(Self(ed25519::PublicKey::from_bytes(bytes)))
    }

    /// 32 바이트 원시 데이터로 인코딩.
    pub fn to_bytes(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }

    /// 서명 검증.
    pub fn verify(&self, msg: &[u8], sig: &Signature) -> Result<()> {
        ed25519::verify(msg, &sig.0, &self.0).map_err(map_ed25519_err)
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
pub struct Signature(ed25519::Signature);

impl Signature {
    /// 64 바이트 원시 데이터로부터 파싱.
    pub fn from_bytes(bytes: &[u8; SIGNATURE_LENGTH]) -> Self {
        Self(ed25519::Signature::from_bytes(bytes))
    }

    /// 64 바이트 원시 데이터로 렌더.
    pub fn to_bytes(&self) -> [u8; SIGNATURE_LENGTH] {
        *self.0.as_bytes()
    }

    /// 소문자 hex (128자).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.as_bytes())
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
            ser.serialize_bytes(self.0.as_bytes())
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

fn map_ed25519_err(e: Ed25519Error) -> Error {
    Error::Crypto(format!("signature: {e:?}"))
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
}
