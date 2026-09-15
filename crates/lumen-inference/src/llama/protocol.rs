//! `llama-server` 네이티브 HTTP API 의 와이어 타입.
//!
//! OpenAI 호환 `/v1/*` 대신 네이티브 `/completion` 을 사용합니다. 네이티브
//! API 만이 `seed`, GBNF `grammar`, 토큰 ID (`return_tokens`), 슬롯 캐시
//! 제어 (`cache_prompt`) 를 모두 노출하며, 이들은 결정론과 도구 호출 제약에
//! 필요합니다. 알 수 없는 필드는 모두 무시하도록 `deny_unknown_fields` 를
//! 쓰지 않습니다 (서버 버전 간 호환).

use serde::{Deserialize, Serialize};

use crate::{FinishReason, SamplingParams};

/// `POST /completion` 요청 바디.
#[derive(Clone, Debug, Serialize)]
pub struct CompletionRequest<'a> {
    /// 프롬프트 텍스트.
    pub prompt: &'a str,
    /// 최대 생성 토큰 수.
    pub n_predict: u32,
    /// 온도.
    pub temperature: f32,
    /// top-k (0 = 비활성).
    pub top_k: u32,
    /// top-p (1.0 = 비활성).
    pub top_p: f32,
    /// 반복 패널티.
    pub repeat_penalty: f32,
    /// 시드. `None` 이면 서버가 선택.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// 중지 시퀀스.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    /// SSE 스트리밍 여부.
    pub stream: bool,
    /// 슬롯 프롬프트 캐시 재사용 여부.
    pub cache_prompt: bool,
    /// 응답에 토큰 ID 포함.
    pub return_tokens: bool,
    /// 토큰별 상위 확률 개수 (0 = 없음).
    pub n_probs: u32,
    /// GBNF 문법.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar: Option<&'a str>,
}

impl<'a> CompletionRequest<'a> {
    /// [`SamplingParams`] 로부터 구성.
    pub fn from_params(
        prompt: &'a str,
        params: &SamplingParams,
        stream: bool,
        cache_prompt: bool,
        grammar: Option<&'a str>,
    ) -> Self {
        Self {
            prompt,
            n_predict: params.max_tokens,
            temperature: params.temperature,
            top_k: params.top_k,
            top_p: params.top_p,
            repeat_penalty: params.repetition_penalty,
            seed: params.seed,
            stop: params.stop_sequences.clone(),
            stream,
            cache_prompt,
            return_tokens: true,
            n_probs: 0,
            grammar,
        }
    }
}

/// `/completion` 응답 (비스트리밍 전체, 또는 스트리밍 청크 하나).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CompletionChunk {
    /// 생성 텍스트 조각 (비스트리밍이면 전체).
    #[serde(default)]
    pub content: String,
    /// 이 청크에 해당하는 토큰 ID.
    #[serde(default)]
    pub tokens: Vec<u32>,
    /// 생성 종료 여부.
    #[serde(default)]
    pub stop: bool,
    /// 종료 종류 (`eos` / `limit` / `word` / `none`). 최신 서버.
    #[serde(default)]
    pub stop_type: Option<String>,
    /// 레거시 종료 플래그.
    #[serde(default)]
    pub stopped_eos: bool,
    /// 레거시 종료 플래그.
    #[serde(default)]
    pub stopped_limit: bool,
    /// 레거시 종료 플래그.
    #[serde(default)]
    pub stopped_word: bool,
    /// 컨텍스트 초과로 프롬프트가 잘렸는지.
    #[serde(default)]
    pub truncated: bool,
    /// 생성된 토큰 수 (마지막 청크).
    #[serde(default)]
    pub tokens_predicted: u32,
}

impl CompletionChunk {
    /// 종료 이유. `stop == false` 이면 `None`.
    pub fn finish_reason(&self) -> Option<FinishReason> {
        if !self.stop {
            return None;
        }
        match self.stop_type.as_deref() {
            Some("eos") => Some(FinishReason::Eos),
            Some("limit") => Some(FinishReason::MaxTokens),
            Some("word") => Some(FinishReason::StopSequence),
            Some(_) => Some(FinishReason::Eos),
            None => {
                if self.stopped_word {
                    Some(FinishReason::StopSequence)
                } else if self.stopped_limit {
                    Some(FinishReason::MaxTokens)
                } else {
                    Some(FinishReason::Eos)
                }
            }
        }
    }
}

/// `GET /health` 응답.
#[derive(Clone, Debug, Deserialize)]
pub struct Health {
    /// `"ok"` 이면 준비 완료.
    #[serde(default)]
    pub status: String,
}

/// `GET /props` 응답 (필요한 필드만).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Props {
    /// 서버가 로드한 모델 파일 경로.
    #[serde(default)]
    pub model_path: Option<String>,
    /// 병렬 슬롯 수.
    #[serde(default)]
    pub total_slots: Option<u32>,
    /// 기본 생성 설정.
    #[serde(default)]
    pub default_generation_settings: Option<GenerationSettings>,
    /// 빌드 정보 문자열.
    #[serde(default)]
    pub build_info: Option<String>,
}

/// `/props.default_generation_settings` 중 필요한 필드.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GenerationSettings {
    /// 컨텍스트 길이.
    #[serde(default)]
    pub n_ctx: Option<u32>,
}

/// 서버 에러 바디.
#[derive(Clone, Debug, Deserialize)]
pub struct ErrorBody {
    /// 에러 상세.
    pub error: ErrorDetail,
}

/// 서버 에러 상세.
#[derive(Clone, Debug, Deserialize)]
pub struct ErrorDetail {
    /// HTTP 코드.
    #[serde(default)]
    pub code: u16,
    /// 메시지.
    #[serde(default)]
    pub message: String,
    /// 종류.
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_only_set_fields() {
        let params = SamplingParams {
            seed: None,
            ..Default::default()
        };
        let req = CompletionRequest::from_params("hi", &params, false, false, None);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["prompt"], "hi");
        assert_eq!(json["n_predict"], 256);
        assert!(json.get("seed").is_none());
        assert!(json.get("stop").is_none());
        assert!(json.get("grammar").is_none());
        assert_eq!(json["return_tokens"], true);

        let params = SamplingParams {
            seed: Some(7),
            stop_sequences: vec!["\n".into()],
            ..Default::default()
        };
        let req = CompletionRequest::from_params("hi", &params, true, true, Some("root ::= \"x\""));
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["seed"], 7);
        assert_eq!(json["stop"][0], "\n");
        assert_eq!(json["grammar"], "root ::= \"x\"");
        assert_eq!(json["stream"], true);
    }

    #[test]
    fn finish_reason_new_and_legacy() {
        let c: CompletionChunk =
            serde_json::from_str(r#"{"content":"a","stop":true,"stop_type":"limit"}"#).unwrap();
        assert_eq!(c.finish_reason(), Some(FinishReason::MaxTokens));
        let c: CompletionChunk =
            serde_json::from_str(r#"{"content":"a","stop":true,"stopped_word":true}"#).unwrap();
        assert_eq!(c.finish_reason(), Some(FinishReason::StopSequence));
        let c: CompletionChunk = serde_json::from_str(r#"{"content":"a","stop":true}"#).unwrap();
        assert_eq!(c.finish_reason(), Some(FinishReason::Eos));
        let c: CompletionChunk = serde_json::from_str(r#"{"content":"a","stop":false}"#).unwrap();
        assert_eq!(c.finish_reason(), None);
    }

    #[test]
    fn props_tolerates_unknown_fields() {
        let p: Props = serde_json::from_str(
            r#"{"model_path":"/m.gguf","total_slots":1,"chat_template":"x","default_generation_settings":{"n_ctx":4096,"seed":1}}"#,
        )
        .unwrap();
        assert_eq!(p.model_path.as_deref(), Some("/m.gguf"));
        assert_eq!(p.default_generation_settings.unwrap().n_ctx, Some(4096));
    }
}
