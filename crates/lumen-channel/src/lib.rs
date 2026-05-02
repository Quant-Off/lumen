//! 비동기 보안 채널.
//!
//! 프로덕션 배포에서 WASM 샌드박스 안의 에이전트와 호스트 TEE 는 *보안
//! 채널* 로 서로 통신합니다 - postcard 인코딩된 타입 메시지를 운반하는
//! 프레임 바이트, 그리고 cross-process / cross-host transport 에 대한
//! 선택적 AEAD 암호화. 이 크레이트가 제공하는 것:
//!
//! - [`SecureChannel`] - 모든 transport 가 구현하는 trait.
//! - [`InProcChannel`] - tokio mpsc 기반 in-process 채널 (오케스트레이터와
//!   단위 테스트가 사용). 항상 사용 가능.
//! - `attested` - software-attested 변종 (Ed25519 상호 핸드셰이크 + 서명된
//!   프레임). TEE 변종은 [`lumen_attestation`] 와 통합.
//! - `encrypted` - AES-256-GCM + x25519 wrapper, `crypto-channel` feature
//!   뒤. v0 에선 의도적으로 최소 - cross-process KEX 경로는 `unimplemented!()`
//!   로 표시됨.
//! - [`tee::TeeChannel`] - 호스트-TEE 보안 채널의 stub. TEE 백엔드가
//!   연결될 때까지 `Error::NotImplemented` 를 반환.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod attested;
pub mod codec;
#[cfg(feature = "crypto-channel")]
pub mod encrypted;
pub mod inproc;
pub mod tee;

use async_trait::async_trait;
use lumen_core::Result;
use serde::{de::DeserializeOwned, Serialize};

/// 타입 양방향 비동기 채널.
///
/// 구현체는 불투명한 byte 프레임 전달을 책임집니다. [`SecureChannel::send`]
/// 와 [`SecureChannel::recv`] 가 그 위에 postcard 인코딩 계층을 더합니다.
#[async_trait]
pub trait SecureChannel: Send {
    /// 원시 byte 프레임 송신.
    async fn send_bytes(&mut self, bytes: Vec<u8>) -> Result<()>;
    /// 원시 byte 프레임 수신.
    async fn recv_bytes(&mut self) -> Result<Vec<u8>>;

    /// 타입 메시지를 postcard 인코딩으로 송신.
    async fn send<T: Serialize + Send + Sync>(&mut self, msg: &T) -> Result<()> {
        let bytes = codec::encode(msg)?;
        self.send_bytes(bytes).await
    }

    /// 타입 메시지를 postcard 디코딩으로 수신.
    async fn recv<T: DeserializeOwned>(&mut self) -> Result<T> {
        let bytes = self.recv_bytes().await?;
        codec::decode::<T>(&bytes)
    }
}
