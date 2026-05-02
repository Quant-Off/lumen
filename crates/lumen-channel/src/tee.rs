//! 호스트-TEE 보안 채널 - v0 에서 자리표시자.
//!
//! 프로덕션에서는 GPU 가속 LLM 이 Intel TDX / AMD SEV-SNP / Apple Secure
//! Enclave 컨텍스트 안에서 실행되고, WASM 샌드박스 안의 에이전트와 postcard
//! 프레임을 주고받습니다. 진짜 구현은 attested TLS 와 유사한 핸드셰이크를
//! 수행한 뒤 프레임을 교환합니다. 그 백엔드가 도착하기 전까지는 모든 연산이
//! [`lumen_core::Error::NotImplemented`] 를 반환합니다.

use async_trait::async_trait;
use lumen_core::{Error, Result};

use crate::SecureChannel;

/// stub 호스트-TEE 채널. 모든 연산에서 `NotImplemented` 반환.
pub struct TeeChannel {
    _private: (),
}

impl TeeChannel {
    /// stub 생성. 호출 사이트가 컴파일되고 v0 빌드에서 명확한
    /// `NotImplemented` 에러를 받도록 하기 위함.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for TeeChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecureChannel for TeeChannel {
    async fn send_bytes(&mut self, _bytes: Vec<u8>) -> Result<()> {
        Err(Error::NotImplemented("TeeChannel::send_bytes"))
    }

    async fn recv_bytes(&mut self) -> Result<Vec<u8>> {
        Err(Error::NotImplemented("TeeChannel::recv_bytes"))
    }
}
