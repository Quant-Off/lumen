//! 모든 채널 transport 가 사용하는 postcard 코덱.

use lumen_core::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};

/// 값을 postcard byte vector 로 인코딩.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    postcard::to_allocvec(value).map_err(|e| Error::Decode(format!("postcard encode: {e}")))
}

/// postcard byte 슬라이스를 값으로 디코딩.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    postcard::from_bytes(bytes).map_err(|e| Error::Decode(format!("postcard decode: {e}")))
}
