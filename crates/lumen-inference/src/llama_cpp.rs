//! llama.cpp 백엔드 스텁 — `llama-cpp` feature 가 활성화된 경우만 컴파일.
//!
//! ## 상태
//!
//! 현재 인터페이스 정의만 제공합니다. 완전한 구현은 `llama-cpp-2` 크레이트
//! 연동으로 v0.5 에서 제공될 예정입니다. llama.cpp 는 cmake 빌드가 필요하며
//! GPU (CUDA / Metal) 가속을 지원합니다.
//!
//! ## llama.cpp 를 선택하는 이유
//!
//! - candle-transformers 대비 더 광범위한 GGUF 모델 지원
//! - CPU / CUDA / Metal 모두에서 최적화된 커널
//! - `n_threads` 조절을 통한 세밀한 자원 관리

use async_trait::async_trait;
use lumen_core::{Error, Result};

use crate::loader::VerifiedModelHandle;
use crate::streaming::{StreamingEngine, TokenStream};
use crate::{Completion, InferenceEngine, SamplingParams};

/// llama.cpp 기반 LLM 추론 엔진 (인터페이스 스텁).
///
/// `llama-cpp-2` 크레이트가 연동되면 이 구조체는 실제 llama.cpp 컨텍스트를
/// 보유합니다.
pub struct LlamaCppEngine {
    /// 검증된 모델 핸들 (경로 + 메타데이터).
    #[allow(dead_code)]
    handle_info: String,
    /// CPU 스레드 수.
    #[allow(dead_code)]
    n_threads: u32,
    /// KV 캐시 컨텍스트 길이.
    #[allow(dead_code)]
    n_ctx: u32,
}

impl LlamaCppEngine {
    /// 검증된 핸들로부터 엔진 설정을 구성합니다.
    ///
    /// 실제 llama.cpp 초기화는 v0.5 에서 구현됩니다.
    pub fn from_verified(handle: &VerifiedModelHandle, n_threads: u32, n_ctx: u32) -> Result<Self> {
        Ok(Self {
            handle_info: format!("{} @ {:?}", handle.model_info.name, handle.path()),
            n_threads,
            n_ctx,
        })
    }
}

#[async_trait]
impl InferenceEngine for LlamaCppEngine {
    async fn complete(&self, _prompt: &str, _params: &SamplingParams) -> Result<Completion> {
        Err(Error::NotImplemented(
            "LlamaCppEngine: llama-cpp-2 연동이 v0.5 에서 구현됩니다".into(),
        ))
    }
}

#[async_trait]
impl StreamingEngine for LlamaCppEngine {
    async fn stream_complete(
        &self,
        _prompt: &str,
        _params: &SamplingParams,
    ) -> Result<TokenStream> {
        Err(Error::NotImplemented(
            "LlamaCppEngine: 스트리밍은 v0.5 에서 구현됩니다".into(),
        ))
    }
}
