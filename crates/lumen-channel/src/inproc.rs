//! tokio MPSC 기반 in-process 채널.

use async_trait::async_trait;
use lumen_core::{Error, Result};
use tokio::sync::mpsc;

use crate::SecureChannel;

/// In-process 채널 한쪽. [`pair`] 로 짝을 맺습니다.
pub struct InProcChannel {
    tx: mpsc::Sender<Vec<u8>>,
    rx: mpsc::Receiver<Vec<u8>>,
}

impl InProcChannel {
    /// 방향당 기본 버퍼 깊이.
    pub const DEFAULT_BUFFER: usize = 32;
}

/// 짝을 이룬 in-process 채널 한 쌍을 생성합니다.
///
/// 두 절반은 거울상이며, 어느 쪽이 [`SecureChannel::send`] 를 호출하면
/// 반대쪽 [`SecureChannel::recv`] 가 메시지를 수신합니다.
pub fn pair() -> (InProcChannel, InProcChannel) {
    pair_with_buffer(InProcChannel::DEFAULT_BUFFER)
}

/// 사용자 지정 버퍼 깊이로 한 쌍을 생성합니다.
pub fn pair_with_buffer(buffer: usize) -> (InProcChannel, InProcChannel) {
    let (a_tx, b_rx) = mpsc::channel(buffer);
    let (b_tx, a_rx) = mpsc::channel(buffer);
    (
        InProcChannel { tx: a_tx, rx: a_rx },
        InProcChannel { tx: b_tx, rx: b_rx },
    )
}

#[async_trait]
impl SecureChannel for InProcChannel {
    async fn send_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.tx
            .send(bytes)
            .await
            .map_err(|_| Error::Channel("peer closed (send)".into()))
    }

    async fn recv_bytes(&mut self) -> Result<Vec<u8>> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| Error::Channel("peer closed (recv)".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecureChannel;

    #[tokio::test]
    async fn typed_roundtrip() {
        let (mut a, mut b) = pair();
        a.send(&("hello", 42u64)).await.unwrap();
        let got: (String, u64) = b.recv().await.unwrap();
        assert_eq!(got.0, "hello");
        assert_eq!(got.1, 42);
    }

    #[tokio::test]
    async fn bidirectional() {
        let (mut a, mut b) = pair();
        a.send(&1u32).await.unwrap();
        b.send(&2u32).await.unwrap();
        let from_a: u32 = b.recv().await.unwrap();
        let from_b: u32 = a.recv().await.unwrap();
        assert_eq!(from_a, 1);
        assert_eq!(from_b, 2);
    }

    #[tokio::test]
    async fn closed_peer_errors() {
        let (mut a, b) = pair();
        drop(b);
        let res: lumen_core::Result<u32> = a.recv().await;
        assert!(res.is_err());
    }
}
