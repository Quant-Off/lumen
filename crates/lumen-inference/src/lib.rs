//! 추론 엔진 추상화.
//!
//! 에이전트 런타임은 매 step 마다 [`InferenceEngine`] 을 호출합니다. 엔진은
//! 텍스트 프롬프트를 자유 형식 completion 또는 에이전트 등록부에서 선택된
//! 구조화된 [`ToolCall`] 로 변환할 책임을 집니다.
//!
//! 프로덕션 배포에서는 무거운 LLM 이 호스트의 TEE 에서 GPU 가속으로
//! 실행되며, WASM 샌드박스 내 에이전트는 [`tee_channel`] (즉
//! [`lumen_channel::SecureChannel`] 위 forward) 로 거기에 도달합니다.
//!
//! ## 백엔드 선택
//!
//! | 백엔드           | feature      | 용도                             |
//! |------------------|--------------|----------------------------------|
//! | `DummyEngine`    | *(없음)*     | 결정론적 테스트 / 데모            |
//! | `CandleEngine`   | `candle`     | ONNX 도구 라우팅                  |
//! | `LlamaCppEngine` | `llama-cpp`  | llama.cpp 연동 (v0.5 예정)        |
//! | `ChannelEngine`  | *(없음)*     | TEE 채널 forward                  |
//!
//! [`BackendConfig`] + [`backend::create_engine`] 으로 팩토리 패턴을 사용할
//! 수 있습니다.
//!
//! ## 폐쇄형(Air-Gapped) 환경 메모
//!
//! HuggingFace 의 `tokenizers` 크레이트와 `candle-transformers` 의 GGUF
//! 텍스트 생성 백엔드는 외부 다운로드 코드를 포함하고 의존 트리가 매우
//! 커서 폐쇄망 빌드 부담이 큽니다. 따라서 v0.4 부터 `candle-llm` feature
//! 는 제거되었습니다. 전체 LLM 텍스트 생성이 필요한 경우 `llama-cpp`
//! (llama.cpp 백엔드, 내장 BPE / SentencePiece 토크나이저) 또는
//! 호스트 TEE 의 추론 서비스로 [`ChannelEngine`] 을 통해 forward 하는
//! 경로를 사용하세요.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "candle")]
pub mod candle;
#[cfg(feature = "llama-cpp")]
pub mod llama_cpp;

pub mod backend;
pub mod dummy;
pub mod loader;
pub mod quantize;
pub mod streaming;
pub mod tee_channel;

use async_trait::async_trait;
use lumen_core::{Result, ToolId};
use serde::{Deserialize, Serialize};

/// 단일 completion 요청의 sampling 파라미터.
///
/// 모든 백엔드가 모든 파라미터를 지원하는 것은 아닙니다. 지원되지 않는
/// 파라미터는 백엔드가 무시합니다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplingParams {
    /// 생성할 최대 토큰 수 (hard cap).
    pub max_tokens: u32,
    /// 샘플링 온도. `0.0` = greedy decoding.
    ///
    /// 높을수록 다양성 증가, 낮을수록 결정론적. 권장 범위: 0.0–2.0.
    pub temperature: f32,
    /// Nucleus sampling 확률 (0, 1) 범위. `1.0` = 비활성.
    ///
    /// 누적 확률이 `top_p` 에 도달하는 상위 토큰만 유지합니다.
    pub top_p: f32,
    /// Top-k sampling. `0` = 비활성.
    ///
    /// 상위 `k` 개 토큰만 후보로 유지합니다.
    pub top_k: u32,
    /// 반복 패널티. `1.0` = 비활성. `> 1.0` 이면 이미 생성된 토큰 억제.
    pub repetition_penalty: f32,
    /// 백엔드가 지원하는 경우의 시드. `None` 이면 백엔드가 임의 시드 선택.
    pub seed: Option<u64>,
    /// 생성을 중지할 시퀀스 목록. 일치 시 [`FinishReason::StopSequence`] 로 종료.
    #[serde(default)]
    pub stop_sequences: Vec<String>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            // dummy 엔진이 기본 설정에서 완전 결정론적이도록 시드를 핀 합니다.
            seed: Some(0),
            temperature: 0.0,
            top_p: 1.0,
            top_k: 0,
            repetition_penalty: 1.0,
            stop_sequences: Vec::new(),
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

pub use backend::BackendConfig;
pub use dummy::DummyEngine;
pub use loader::{VerifiedModelHandle, VerifiedModelLoader};
pub use quantize::{GgufLevel, QuantizationConfig, QuantizationKind};
pub use streaming::{FinishReason, StreamingEngine, Token, TokenStream};
pub use tee_channel::ChannelEngine;

#[cfg(feature = "llama-cpp")]
pub use llama_cpp::LlamaCppEngine;
