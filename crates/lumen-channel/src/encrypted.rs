//! AES-256-GCM + x25519 채널 wrapper. v0 에서 **stub**.
//!
//! 의도는 임의의 [`SecureChannel`] transport 위에 confidential, authenticated
//! 채널 계층을 제공하는 것입니다. 핸드셰이크 (x25519 KEX, HKDF 도출,
//! 리플레이 윈도우, 키 회전) 는 그 자체로 상당한 작업이므로 - 그때까지는
//! 인스턴스화하면 [`lumen_core::Error::NotImplemented`] 를 반환해 호출
//! 사이트가 조용한 보안 폴백 대신 시끄러운 감사 가능 신호를 받게 합니다.

use async_trait::async_trait;
use lumen_core::{Error, Result};

use crate::SecureChannel;

/// 암호화 채널 wrapper.
pub struct EncryptedChannel<C: SecureChannel> {
    inner: C,
}

impl<C: SecureChannel> EncryptedChannel<C> {
    /// 내부 transport 를 wrap. KEX 는 미구현이며, 다운스트림 소비자가 API
    /// 표면을 컴파일할 수 있도록 생성자 시그니처만 유지됩니다.
    pub fn new(_inner: C) -> Result<Self> {
        Err(Error::NotImplemented("EncryptedChannel::new"))
    }

    /// 해체 후 내부 transport 를 반환.
    pub fn into_inner(self) -> C {
        self.inner
    }
}

#[async_trait]
impl<C: SecureChannel> SecureChannel for EncryptedChannel<C> {
    async fn send_bytes(&mut self, _bytes: Vec<u8>) -> Result<()> {
        Err(Error::NotImplemented("EncryptedChannel::send_bytes"))
    }

    async fn recv_bytes(&mut self) -> Result<Vec<u8>> {
        Err(Error::NotImplemented("EncryptedChannel::recv_bytes"))
    }
}
