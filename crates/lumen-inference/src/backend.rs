//! 추론 백엔드 팩토리.
//!
//! [`BackendConfig`] 열거형으로 원하는 백엔드를 선언하고,
//! [`create_engine`] 으로 [`InferenceEngine`] trait 객체를 얻습니다.
//!
//! 각 백엔드는 개별 feature 로 보호됩니다:
//!
//! | 백엔드             | feature      | 설명                                 |
//! |--------------------|--------------|--------------------------------------|
//! | `Dummy`            | *(없음)*     | 결정론적 패턴 매칭, 테스트 전용.      |
//! | `CandleOnnx`       | `candle`     | ONNX 라우팅 (기존 `CandleEngine`).   |
//! | `CandleLlm`        | `candle-llm` | GGUF LLM (candle-transformers).      |
//! | `LlamaCpp`         | `llama-cpp`  | llama.cpp 스텁 (v0.5 완성 예정).     |

#[cfg(any(feature = "candle", feature = "candle-llm", feature = "llama-cpp"))]
use crate::loader::VerifiedModelHandle;
#[cfg(feature = "candle-llm")]
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::Result;

use crate::{DummyEngine, InferenceEngine};

/// 원하는 추론 백엔드와 설정을 기술하는 열거형.
#[non_exhaustive]
pub enum BackendConfig {
    /// 테스트 및 데모용 결정론적 패턴-매칭 엔진.
    Dummy,

    /// ONNX 모델 기반 도구 라우팅 엔진.
    ///
    /// `candle` feature 가 활성화되어야 합니다.
    #[cfg(feature = "candle")]
    CandleOnnx {
        /// 검증된 `.onnx` 모델 핸들.
        handle: VerifiedModelHandle,
        /// argmax 인덱스 → ToolId 매핑 테이블.
        tool_table: Vec<lumen_core::ToolId>,
    },

    /// GGUF 양자화 LLM (candle-transformers 기반).
    ///
    /// `candle-llm` feature 가 활성화되어야 합니다.
    #[cfg(feature = "candle-llm")]
    CandleLlm {
        /// 검증된 `.gguf` 모델 핸들.
        handle: VerifiedModelHandle,
        /// HuggingFace `tokenizer.json` 경로.
        tokenizer_path: PathBuf,
    },

    /// llama.cpp 기반 엔진 (v0.5 완성 예정).
    ///
    /// `llama-cpp` feature 가 활성화되어야 합니다.
    #[cfg(feature = "llama-cpp")]
    LlamaCpp {
        /// 검증된 모델 핸들.
        handle: VerifiedModelHandle,
        /// CPU 스레드 수.
        n_threads: u32,
        /// KV 캐시 컨텍스트 길이.
        n_ctx: u32,
    },
}

/// `config` 에 따라 추론 엔진 trait 객체를 생성합니다.
///
/// 반환값은 `Arc<dyn InferenceEngine>` 이므로 [`AgentRuntime`] 에 바로 전달
/// 가능합니다.
///
/// [`AgentRuntime`]: lumen_agent::AgentRuntime
pub fn create_engine(config: BackendConfig) -> Result<Arc<dyn InferenceEngine>> {
    match config {
        BackendConfig::Dummy => Ok(Arc::new(DummyEngine::new())),

        #[cfg(feature = "candle")]
        BackendConfig::CandleOnnx { handle, tool_table } => {
            use crate::candle::CandleEngine;
            Ok(Arc::new(CandleEngine::load(handle.path(), tool_table)?))
        }

        #[cfg(feature = "candle-llm")]
        BackendConfig::CandleLlm {
            handle,
            tokenizer_path,
        } => {
            use crate::candle_llm::CandleLlmEngine;
            Ok(Arc::new(CandleLlmEngine::from_gguf(&handle, tokenizer_path)?))
        }

        #[cfg(feature = "llama-cpp")]
        BackendConfig::LlamaCpp {
            handle,
            n_threads,
            n_ctx,
        } => {
            use crate::llama_cpp::LlamaCppEngine;
            Ok(Arc::new(LlamaCppEngine::from_verified(&handle, n_threads, n_ctx)?))
        }
    }
}
