//! Ed25519 래퍼 + serde 글루.
//!
//! `lumen-core` 가 `ed25519-dalek` 에 직접 의존하는 유일한 크레이트가 되어야
//! 합니다. Capability 토큰과 provenance 서명 모두 이 타입들을 거칩니다.

use std::fmt;

use ed25519_dalek::{
    Signer, SigningKey as DalekSigningKey, Verifier, VerifyingKey as DalekVerifyingKey,
    SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};

/// Ed25519 서명 (개인) 키.
///
/// `ed25519_dalek::SigningKey` 를 감쌉니다. **절대 직렬화되지 않습니다** -
/// 개인 키 export 는 명시적이고 감사된 작업이어야 합니다. 그 이유로 래퍼는
/// 의도적으로 `Serialize` / `Display` impl 을 두지 않습니다.
#[derive(Clone)]
pub struct SigningKey(DalekSigningKey);

impl SigningKey {
    /// CSPRNG 으로부터 새 키를 생성합니다.
    pub fn generate<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        Self(DalekSigningKey::generate(rng))
    }

    /// 32 바이트 원시 시드로부터 생성.
    pub fn from_seed(seed: &[u8; SECRET_KEY_LENGTH]) -> Self {
        Self(DalekSigningKey::from_bytes(seed))
    }

    /// 시드 바이트를 반환 (민감 - 로그에 남기지 마세요).
    pub fn to_seed(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.0.to_bytes()
    }

    /// 공개 검증 키를 도출합니다.
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(self.0.verifying_key())
    }

    /// 메시지에 서명합니다.
    pub fn sign(&self, msg: &[u8]) -> Signature {
        Signature(self.0.sign(msg))
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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerifyingKey(DalekVerifyingKey);

impl VerifyingKey {
    /// 32 바이트 원시 데이터로부터 파싱.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        DalekVerifyingKey::from_bytes(bytes)
            .map(Self)
            .map_err(|e| Error::Crypto(format!("verifying key: {e}")))
    }

    /// 32 바이트 원시 데이터로 인코딩.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// 서명 검증.
    pub fn verify(&self, msg: &[u8], sig: &Signature) -> Result<()> {
        self.0
            .verify(msg, &sig.0)
            .map_err(|e| Error::Crypto(format!("signature: {e}")))
    }

    /// 64자 소문자 hex 로 렌더.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
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
            ser.serialize_bytes(&self.0.to_bytes())
        }
    }
}

impl<'de> Deserialize<'de> for VerifyingKey {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        if de.is_human_readable() {
            let s = String::deserialize(de)?;
            let mut out = [0u8; 32];
            hex::decode_to_slice(s.trim(), &mut out)
                .map_err(serde::de::Error::custom)?;
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
pub struct Signature(ed25519_dalek::Signature);

impl Signature {
    /// 64 바이트 원시 데이터로부터 파싱.
    pub fn from_bytes(bytes: &[u8; SIGNATURE_LENGTH]) -> Self {
        Self(ed25519_dalek::Signature::from_bytes(bytes))
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
            hex::decode_to_slice(s.trim(), &mut out)
                .map_err(serde::de::Error::custom)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

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
