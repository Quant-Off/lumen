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
//! ## 계층 구조
//!
//! ```text
//! AgentRuntime
//!   |- InferenceEngine   (complete)        <- 모든 백엔드가 구현
//!   |- StreamingEngine   (stream_complete) <- 선택 구현
//!   `- EngineInfo        (info)            <- 런타임이 백엔드 능력을 질의
//!
//! backend::Engines / BackendRegistry  <- 이름 기반 팩토리, 외부 크레이트 확장점
//!   |- DummyEngine                   (테스트)
//!   |- ChannelEngine                 (SecureChannel 위 TEE forward)
//!   `- llama::LlamaServerEngine      (feature `llama-server`)
//! ```
//!
//! 새 추론 엔진을 올리려면 [`InferenceEngine`] (+ 필요 시 [`StreamingEngine`])
//! 만 구현하고 [`backend::BackendRegistry`] 에 이름으로 등록하면 됩니다.
//! 런타임 / CLI / TEE peer 는 trait 객체만 다루므로 코드 변경이 없습니다.
//!
//! ## 백엔드 선택
//!
//! | 백엔드              | feature        | 용도                                   |
//! |---------------------|----------------|----------------------------------------|
//! | `DummyEngine`       | *(없음)*       | 결정론적 테스트 / 데모                  |
//! | `ChannelEngine`     | *(없음)*       | TEE 채널 forward (샌드박스 -> 호스트)   |
//! | `LlamaServerEngine` | `llama-server` | llama.cpp (`llama-server`) 격리 프로세스 |
//!
//! ## 폐쇄형(Air-Gapped) 환경 메모
//!
//! HuggingFace 의 `tokenizers` 크레이트와 `candle-*` 백엔드는 외부 다운로드
//! 코드를 포함하고 의존 트리가 매우 커서 폐쇄망 빌드 부담이 큽니다. 따라서
//! v0.4 부터 모든 candle feature 는 제거되었습니다. llama.cpp 역시 in-process
//! FFI (`llama-cpp-2`, cmake + bindgen + libclang 필요) 대신 **격리된
//! `llama-server` 프로세스** 를 자체 HTTP/1.1 클라이언트로 연결하는 방식을
//! 택했습니다. Rust 빌드에는 C/C++ 툴체인이 전혀 필요 없고, 엔진 바이너리는
//! 모델 파일과 동일하게 BLAKE3 핀으로 검증됩니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod backend;
pub mod dummy;
#[cfg(feature = "llama-server")]
pub mod http;
#[cfg(feature = "llama-server")]
pub mod llama;
pub mod loader;
pub mod quantize;
pub mod streaming;
pub mod tee_channel;
pub mod tokenizer;
pub mod toolcall;

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

/// 백엔드가 제공하는 기능 플래그.
///
/// 런타임은 이 값으로 "이 엔진에 문법 제약을 걸 수 있는가", "시드가
/// 결정론을 보장하는가" 등을 판단합니다. 기본값은 모두 `false` 이며, 각
/// 백엔드가 실제 지원 항목만 `true` 로 올립니다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCapabilities {
    /// [`StreamingEngine`] 구현 여부.
    pub streaming: bool,
    /// 동일 (프롬프트, 파라미터, 시드) 에 대해 동일 출력을 보장하는지.
    pub seed_deterministic: bool,
    /// 출력 문법 제약 (GBNF / JSON schema) 지원 여부.
    pub grammar: bool,
    /// 토큰별 로그 확률 제공 여부.
    pub logprobs: bool,
    /// 엔진 자체가 구조화된 [`ToolCall`] 을 생성하는지 (텍스트 파싱이 아닌).
    pub native_tool_calls: bool,
}

/// 백엔드 식별 정보와 기능.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    /// 백엔드 이름 (예: `"dummy"`, `"llama-server"`, `"channel"`).
    pub backend: String,
    /// 로드된 모델 식별자 (경로 / 이름). 백엔드가 모르면 `None`.
    pub model: Option<String>,
    /// 기능 플래그.
    pub capabilities: EngineCapabilities,
    /// 최대 컨텍스트 길이 (토큰). 백엔드가 모르면 `None`.
    pub max_context: Option<u32>,
}

/// 플러거블 추론 백엔드.
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    /// completion 실행.
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion>;

    /// 백엔드 식별 정보와 기능 플래그.
    ///
    /// 기본 구현은 이름 없는 최소 정보를 반환합니다. 새 백엔드는 이 메서드를
    /// 재정의해 런타임이 기능을 질의할 수 있게 하는 것을 권장합니다.
    fn info(&self) -> EngineInfo {
        EngineInfo::default()
    }
}

pub use backend::{BackendConfig, BackendRegistry, Engines};
pub use dummy::DummyEngine;
pub use loader::{VerifiedModelHandle, VerifiedModelLoader};
pub use quantize::{GgufLevel, QuantizationConfig, QuantizationKind};
pub use streaming::{FinishReason, StreamingEngine, Token, TokenStream};
pub use tee_channel::ChannelEngine;
pub use tokenizer::{BpeTokenizer, TokenId};
pub use toolcall::{parse_tool_call, ToolCallGrammar};

#[cfg(feature = "llama-server")]
pub use llama::{Endpoint, LlamaServerConfig, LlamaServerEngine};
