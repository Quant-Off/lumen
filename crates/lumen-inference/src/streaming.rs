//! 토큰 단위 스트리밍 추론 추상화.
//!
//! [`StreamingEngine`] 은 [`crate::InferenceEngine`] 과 독립적인 보조 trait 으로,
//! 구현체가 두 trait 을 모두 구현하는 방식을 취합니다. 런타임은 엔진이
//! `StreamingEngine` 을 구현하는지 확인한 후 토큰 단위 스트리밍을 활성화합니다.
//!
//! 스트림이 완전히 소진되기 전에 drop 하면 생성을 중단합니다
//! (채널 송신자 에러 → background task 종료).

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use lumen_core::Result;
use serde::{Deserialize, Serialize};

use crate::SamplingParams;

/// 단일 생성 토큰.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Token {
    /// 어휘 ID (모델 vocabulary 인덱스).
    pub id: u32,
    /// 디코딩된 UTF-8 조각. BPE 바이트 폴백 포함.
    pub text: String,
    /// 로그 확률 (백엔드가 지원하는 경우).
    pub logprob: Option<f32>,
    /// 스트림 종료 이유. 마지막 토큰에만 `Some`.
    pub finish_reason: Option<FinishReason>,
}

/// 생성 루프 종료 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    /// `max_tokens` 상한에 도달했습니다.
    MaxTokens,
    /// End-of-sequence 토큰이 생성되었습니다.
    Eos,
    /// `stop_sequences` 중 하나가 일치했습니다.
    StopSequence,
}

/// 동적 디스패치를 위한 핀 박스형 토큰 스트림.
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<Token>> + Send>>;

/// 토큰 단위 스트리밍을 지원하는 추론 백엔드.
///
/// 이 trait 을 구현하지 않는 백엔드는 [`crate::InferenceEngine::complete`] 만
/// 제공하며, 런타임은 그 결과를 한 번에 반환합니다.
#[async_trait]
pub trait StreamingEngine: Send + Sync {
    /// `prompt` 에 대한 토큰 스트림을 반환합니다.
    ///
    /// 반환된 스트림을 소진하면 전체 completion 을 얻을 수 있습니다.
    /// 중간에 drop 하면 생성이 취소됩니다.
    async fn stream_complete(&self, prompt: &str, params: &SamplingParams) -> Result<TokenStream>;
}
