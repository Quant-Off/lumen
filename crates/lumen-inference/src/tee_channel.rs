//! 채널 기반 추론 엔진.
//!
//! 모든 completion 요청을 [`lumen_channel::SecureChannel`] 로 forward 하고
//! 응답을 읽어옵니다. 반대편은 호스트 TEE 가 될 것으로 예상하며, v0 에서는
//! 같은 프로세스 내에서 동일한 `DummyEngine` 을 실행하는 peer 를 함께
//! 제공해 끝-끝 테스트가 wire 형식을 검증할 수 있게 합니다.

use async_trait::async_trait;
use lumen_channel::SecureChannel;
use lumen_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{Completion, InferenceEngine, SamplingParams};

/// 와이어 형식 요청.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferRequest {
    /// 프롬프트 텍스트.
    pub prompt: String,
    /// sampling 컨트롤.
    pub params: SamplingParams,
}

/// 와이어 형식 응답.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferResponse {
    /// completion 또는 에러 문자열.
    pub result: std::result::Result<Completion, String>,
}

/// 호출을 채널로 forward 하는 엔진.
pub struct ChannelEngine<C: SecureChannel> {
    channel: Mutex<C>,
}

impl<C: SecureChannel> ChannelEngine<C> {
    /// 채널을 wrap.
    pub fn new(channel: C) -> Self {
        Self {
            channel: Mutex::new(channel),
        }
    }
}

#[async_trait]
impl<C: SecureChannel> InferenceEngine for ChannelEngine<C> {
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion> {
        let req = InferRequest {
            prompt: prompt.to_string(),
            params: params.clone(),
        };
        let mut guard = self.channel.lock().await;
        guard.send(&req).await?;
        let resp: InferResponse = guard.recv().await?;
        match resp.result {
            Ok(c) => Ok(c),
            Err(e) => Err(Error::Inference(e)),
        }
    }
}

/// `channel` 위에서 inference 요청을 처리하는 "TEE peer" 루프 실행. 같은
/// 프로세스에 양 끝이 있는 테스트와 v0 데모에 사용됩니다.
pub async fn run_peer<C: SecureChannel, E: InferenceEngine>(
    mut channel: C,
    engine: &E,
) -> Result<()> {
    loop {
        let req: InferRequest = match channel.recv().await {
            Ok(r) => r,
            Err(Error::Channel(_)) => return Ok(()), // peer 종료
            Err(e) => return Err(e),
        };
        let result = engine
            .complete(&req.prompt, &req.params)
            .await
            .map_err(|e| e.to_string());
        channel.send(&InferResponse { result }).await?;
    }
}
