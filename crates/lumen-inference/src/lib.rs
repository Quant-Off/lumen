//! 추론 엔진 추상화.
//!
//! 에이전트 런타임은 매 step 마다 [`InferenceEngine`] 를 호출합니다. 엔진은
//! 텍스트 프롬프트를 자유 형식 completion 또는 에이전트 등록부에서 선택된
//! 구조화된 [`ToolCall`] 로 변환할 책임을 집니다.
//!
//! 프로덕션 배포에서는 무거운 LLM 이 호스트의 TEE 에서 GPU 가속으로
//! 실행되며, WASM 샌드박스 내 에이전트는 [`tee_channel`] (즉
//! [`lumen_channel::SecureChannel`] 위 forward) 로 거기에 도달합니다.
//!
//! 단위 테스트, 예제, v0 CLI 데모에는 [`dummy::DummyEngine`] 만으로 충분
//! 합니다 - 완전 결정론적이며 몇 가지 트리거 단어를 패턴 매치해 예측
//! 가능한 [`ToolCall`] 을 발행합니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "candle")]
pub mod candle;
pub mod dummy;
pub mod tee_channel;

use async_trait::async_trait;
use lumen_core::{Result, ToolId};
use serde::{Deserialize, Serialize};

/// 단일 completion 요청의 sampling 파라미터.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplingParams {
    /// 토큰 상한 (hard cap).
    pub max_tokens: u32,
    /// 백엔드가 지원하는 경우의 옵션 32 비트 시드.
    pub seed: Option<u64>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            // dummy 엔진이 기본 설정에서 완전 결정론적이도록 시드를 핀 합니다
            // - 의외의 기본 동작 방지.
            seed: Some(0),
        }
    }
}

/// 모델이 제안한 구조화된 도구 호출.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// 호출할 도구.
    pub id: ToolId,
    /// JSON 객체로 인코딩된 인자.
    pub args_json: String,
}

/// 엔진 출력.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    /// 자유 형식 텍스트 응답 (도구 호출이 발행되었으면 빌 수 있음).
    pub text: String,
    /// 옵션 도구 호출.
    pub tool_call: Option<ToolCall>,
}

/// 플러거블 추론 백엔드.
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    /// completion 실행.
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion>;
}

pub use dummy::DummyEngine;
pub use tee_channel::ChannelEngine;
