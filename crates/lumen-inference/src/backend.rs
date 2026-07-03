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
//! | `LlamaCpp`         | `llama-cpp`  | llama.cpp 연동 (v0.5 완성 예정).     |
//!
//! `candle` (candle-core / candle-onnx) 백엔드는 폐쇄형 환경 호환을 위해
//! v0.4 에서 제거되었습니다. ONNX 도구 라우팅이 필요한 경우 자체 BPE
//! 토크나이저 + 호스트 TEE 의 추론 서비스로 [`crate::ChannelEngine`] 을
//! 통해 forward 하는 경로를 사용하세요. 전체 LLM 텍스트 생성은 `llama-cpp`
//! 백엔드를 사용하세요.

#[cfg(feature = "llama-cpp")]
use crate::loader::VerifiedModelHandle;
use std::sync::Arc;

use lumen_core::Result;

use crate::{DummyEngine, InferenceEngine};

/// 원하는 추론 백엔드와 설정을 기술하는 열거형.
#[non_exhaustive]
pub enum BackendConfig {
    /// 테스트 및 데모용 결정론적 패턴-매칭 엔진.
    Dummy,

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
/// 반환값은 `Arc<dyn InferenceEngine>` 이므로 `AgentRuntime` 에 바로 전달
/// 가능합니다.
pub fn create_engine(config: BackendConfig) -> Result<Arc<dyn InferenceEngine>> {
    match config {
        BackendConfig::Dummy => Ok(Arc::new(DummyEngine::new())),

        #[cfg(feature = "llama-cpp")]
        BackendConfig::LlamaCpp {
            handle,
            n_threads,
            n_ctx,
        } => {
            use crate::llama_cpp::LlamaCppEngine;
            Ok(Arc::new(LlamaCppEngine::from_verified(
                &handle, n_threads, n_ctx,
            )?))
        }
    }
}
