//! 워크스페이스 전체에서 사용되는 newtype 식별자.
//!
//! 모든 binary ID 는 16 바이트 (난수). 32자 소문자 hex 로 렌더되며 임의의
//! 대소문자에서 관대하게 파싱됩니다. ToolID 는 정책 파일을 손으로 작성하기
//! 쉽도록 짧은 사람-친화 문자열입니다 (≤ 64 바이트, ASCII 인쇄 가능, 공백
//! 제외).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};
use crate::rng::Rng;

macro_rules! binary_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub [u8; 16]);

        impl $name {
            #[doc = "암호학적으로 안전한 RNG 로 임의의 ID 를 생성합니다."]
            pub fn random<R: Rng>(rng: &mut R) -> Self {
                let mut bytes = [0u8; 16];
                rng.fill_bytes(&mut bytes);
                Self(bytes)
            }

            #[doc = "고정된 byte 배열로부터 생성합니다."]
            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            #[doc = "원시 바이트 차용."]
            pub fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }

            #[doc = "32자 소문자 hex 로 렌더합니다."]
            pub fn to_hex(&self) -> String {
                hex::encode(self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.to_hex())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self> {
                let mut out = [0u8; 16];
                hex::decode_to_slice(s.trim(), &mut out)?;
                Ok(Self(out))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
                if ser.is_human_readable() {
                    ser.serialize_str(&self.to_hex())
                } else {
                    ser.serialize_bytes(&self.0)
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
                if de.is_human_readable() {
                    let s = String::deserialize(de)?;
                    Self::from_str(&s).map_err(serde::de::Error::custom)
                } else {
                    let bytes = <Vec<u8>>::deserialize(de)?;
                    let arr: [u8; 16] = bytes
                        .as_slice()
                        .try_into()
                        .map_err(|_| serde::de::Error::custom("expected 16-byte id"))?;
                    Ok(Self(arr))
                }
            }
        }
    };
}

binary_id!(AgentId, "에이전트 인스턴스의 식별자. 16 바이트의 난수.");
binary_id!(CapabilityId, "Capability 토큰의 식별자. 16 바이트의 난수.");
binary_id!(
    RequestId,
    "단일 에이전트 요청 / 스텝의 식별자. 16 바이트의 난수."
);

/// [`ToolId`] 의 최대 길이.
pub const TOOL_ID_MAX_LEN: usize = 64;

/// 에이전트에 등록된 도구의 식별자.
///
/// 정책을 손으로 작성하기 쉽도록 짧은 사람-친화 문자열로 도구를 지칭합니다.
/// 정책 모호성을 피하기 위해 엄격한 문자 집합을 강제합니다.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolId(String);

impl ToolId {
    /// 길이와 문자 집합을 검증하면서 새 도구 ID 를 생성합니다.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        if s.is_empty() || s.len() > TOOL_ID_MAX_LEN {
            return Err(Error::Invalid(format!(
                "tool id length must be 1..={TOOL_ID_MAX_LEN}, got {}",
                s.len()
            )));
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        {
            return Err(Error::Invalid("tool id must match [A-Za-z0-9_.-]+".into()));
        }
        Ok(Self(s))
    }

    /// `&str` 로 차용.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ToolId({})", self.0)
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ToolId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::new(s)
    }
}

impl Serialize for ToolId {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        ser.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolId {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::OsRng;

    #[test]
    fn agent_id_hex_roundtrip() {
        let id = AgentId::random(&mut OsRng);
        let s = id.to_hex();
        assert_eq!(s.len(), 32);
        let parsed: AgentId = s.parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn tool_id_validates() {
        ToolId::new("echo").unwrap();
        ToolId::new("file.read").unwrap();
        assert!(ToolId::new("").is_err());
        assert!(ToolId::new(" space ").is_err());
        assert!(ToolId::new("bad/slash").is_err());
        assert!(ToolId::new("a".repeat(TOOL_ID_MAX_LEN + 1)).is_err());
    }
}
