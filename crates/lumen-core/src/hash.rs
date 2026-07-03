//! Lumen 전체에서 사용되는 BLAKE3 기반 해시 프리미티브.
//!
//! 폐쇄형(Air-Gapped) iso-light-k0 환경 호환을 위해 구현체는
//! `elib-k0-nt/blake` 의 [`Blake3`] 를 사용합니다. 외부 `blake3` 크레이트
//! 의존을 제거합니다.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use elib_blake::Blake3;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};

/// 32 바이트 BLAKE3 다이제스트.
///
/// 동등성 비교에는 상수시간 비교를 사용하여 비밀 의존 식별자 (예: capability
/// 다이제스트 매칭) 로 안전하게 사용할 수 있도록 합니다.
#[derive(Clone, Copy)]
pub struct Blake3Hash(pub [u8; 32]);

impl Blake3Hash {
    /// 주어진 byte 슬라이스에 대해 해시를 계산합니다.
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Blake3::new();
        hasher.update(bytes);
        Self(finalize_32(hasher))
    }

    /// 디스크의 파일을 스트리밍으로 점진적으로 해시합니다.
    ///
    /// 64 KiB 청크 단위. 같은 바이트열에 대한 [`Blake3Hash::of`] 와 동일한
    /// 출력을 호스트나 스레딩과 무관하게 보장합니다.
    pub fn of_file(path: &Path) -> std::io::Result<Self> {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Blake3::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(Self(finalize_32(hasher)))
    }

    /// 원시 바이트 차용.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// 64자 소문자 hex 문자열로 렌더.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl PartialEq for Blake3Hash {
    fn eq(&self, other: &Self) -> bool {
        // elib-k0-nt/constant-time 의 상수시간 슬라이스 비교.
        elib_blake::ct_eq_slice(&self.0, &other.0).unwrap_u8() == 1
    }
}

impl Eq for Blake3Hash {}

impl std::hash::Hash for Blake3Hash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for Blake3Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Blake3Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Debug for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blake3Hash({})", self.to_hex())
    }
}

impl fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for Blake3Hash {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let mut out = [0u8; 32];
        hex::decode_to_slice(s.trim(), &mut out)?;
        Ok(Self(out))
    }
}

impl Serialize for Blake3Hash {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        if ser.is_human_readable() {
            ser.serialize_str(&self.to_hex())
        } else {
            ser.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Blake3Hash {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        if de.is_human_readable() {
            let s = String::deserialize(de)?;
            Self::from_str(&s).map_err(serde::de::Error::custom)
        } else {
            let bytes = <Vec<u8>>::deserialize(de)?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| serde::de::Error::custom("blake3 must be 32 bytes"))?;
            Ok(Self(arr))
        }
    }
}

/// postcard 바이너리 인코딩을 통해 직렬화 가능 값에 대해 해시를 계산합니다.
///
/// 출력은 호스트 endianness 와 필드 순회 순서에 무관합니다 - 그 대신
/// `Serialize` 가 필요합니다. witness / 공개 입력을 바인드할 때 유용합니다.
pub fn hash_postcard<T: Serialize>(value: &T) -> Result<Blake3Hash> {
    let bytes = postcard::to_allocvec(value).map_err(|e| Error::Decode(e.to_string()))?;
    Ok(Blake3Hash::of(&bytes))
}

/// `Blake3` 해시를 32 바이트 배열로 종결합니다.
///
/// elib-k0-nt 의 `finalize` 는 메모리 보안을 위해 `SecureBuffer` 를 반환합니다.
/// Lumen 의 식별자는 short-lived 한 32 바이트 다이제스트이므로 즉시 일반 배열로
/// 복사해 사용합니다 (`SecureBuffer` 는 함수 종료와 동시에 zeroize 됨).
fn finalize_32(hasher: Blake3) -> [u8; 32] {
    let buf = hasher.finalize().expect("blake3 finalize failed");
    let mut out = [0u8; 32];
    out.copy_from_slice(buf.as_slice());
    out
}

/// `(key, label, epoch_le_bytes)` 트리플로 32 바이트 키 자료를 도출합니다.
///
/// `lumen-channel` 의 키 도출에서 사용됩니다. BLAKE3 keyed 모드는
/// `key` 자체를 IV 로 사용하므로 키 바이트가 누설되지 않습니다.
pub fn blake3_keyed_derive_32(key: &[u8; 32], label: &[u8], extra: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3::new_keyed(key);
    hasher.update(label);
    hasher.update(extra);
    finalize_32(hasher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinism_same_input_same_hash() {
        let a = Blake3Hash::of(b"lumen");
        let b = Blake3Hash::of(b"lumen");
        assert_eq!(a, b);
    }

    #[test]
    fn hex_roundtrip() {
        let a = Blake3Hash::of(b"x");
        let s = a.to_hex();
        let b: Blake3Hash = s.parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn distinct_inputs_distinct_hashes() {
        assert_ne!(Blake3Hash::of(b"a"), Blake3Hash::of(b"b"));
    }

    #[test]
    fn keyed_derive_distinct_for_different_keys() {
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        let a = blake3_keyed_derive_32(&k1, b"label", b"");
        let b = blake3_keyed_derive_32(&k2, b"label", b"");
        assert_ne!(a, b);
    }
}
