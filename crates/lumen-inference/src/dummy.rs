//! 테스트와 데모용 결정론적 패턴-매칭 추론 엔진.
//!
//! 규칙:
//!
//! - `echo ` (대소문자 구분 안 함) 로 시작하는 프롬프트는 `echo` 도구의
//!   `ToolCall` 을 `{"text": <rest>}` 와 함께 발행합니다.
//! - `add ` 로 시작하는 프롬프트는 `add` 도구의 `ToolCall` 을 다음 두 개의
//!   공백-분리 정수로 파싱한 `{"a": .., "b": ..}` 와 함께 발행합니다.
//! - 그 외에는 결정론적인 텍스트 응답.

use async_trait::async_trait;
use lumen_core::{Result, ToolId};
use serde::Serialize;

use crate::{Completion, InferenceEngine, SamplingParams, ToolCall};

/// Dummy 엔진 - 무상태 결정론적.
#[derive(Clone, Debug, Default)]
pub struct DummyEngine;

impl DummyEngine {
    /// 생성.
    pub fn new() -> Self {
        Self
    }

    fn route(prompt: &str) -> Option<ToolCall> {
        let lower = prompt.trim_start();
        if let Some(rest) = strip_prefix_ascii_ci(lower, "echo ") {
            #[derive(Serialize)]
            struct Args<'a> {
                text: &'a str,
            }
            let args = Args { text: rest.trim() };
            return Some(ToolCall {
                id: ToolId::new("echo").expect("echo is a valid tool id"),
                args_json: serde_json::to_string(&args).expect("trivial json"),
            });
        }
        if let Some(rest) = strip_prefix_ascii_ci(lower, "add ") {
            let mut it = rest.split_whitespace();
            let a = it.next().and_then(|s| s.parse::<i64>().ok())?;
            let b = it.next().and_then(|s| s.parse::<i64>().ok())?;
            #[derive(Serialize)]
            struct Args {
                a: i64,
                b: i64,
            }
            let args = Args { a, b };
            return Some(ToolCall {
                id: ToolId::new("add").expect("add is a valid tool id"),
                args_json: serde_json::to_string(&args).expect("trivial json"),
            });
        }
        None
    }
}

#[async_trait]
impl InferenceEngine for DummyEngine {
    async fn complete(&self, prompt: &str, _params: &SamplingParams) -> Result<Completion> {
        if let Some(call) = Self::route(prompt) {
            Ok(Completion {
                text: String::new(),
                tool_call: Some(call),
            })
        } else {
            Ok(Completion {
                text: format!(
                    "[dummy reply] {}",
                    prompt.chars().take(120).collect::<String>()
                ),
                tool_call: None,
            })
        }
    }
}

fn strip_prefix_ascii_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() {
        return None;
    }
    let (head, tail) = s.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_routes_to_tool() {
        let e = DummyEngine::new();
        let c = e
            .complete("echo hi there", &SamplingParams::default())
            .await
            .unwrap();
        let call = c.tool_call.expect("expected tool call");
        assert_eq!(call.id.as_str(), "echo");
        assert!(call.args_json.contains("\"hi there\""));
    }

    #[tokio::test]
    async fn add_parses_two_ints() {
        let e = DummyEngine::new();
        let c = e
            .complete("add 3 4", &SamplingParams::default())
            .await
            .unwrap();
        let call = c.tool_call.expect("expected tool call");
        assert_eq!(call.id.as_str(), "add");
        assert!(call.args_json.contains("\"a\":3"));
        assert!(call.args_json.contains("\"b\":4"));
    }

    #[tokio::test]
    async fn determinism() {
        let e = DummyEngine::new();
        let a = e
            .complete("echo same", &SamplingParams::default())
            .await
            .unwrap();
        let b = e
            .complete("echo same", &SamplingParams::default())
            .await
            .unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn fallback_text() {
        let e = DummyEngine::new();
        let c = e
            .complete("Summarise the meeting notes.", &SamplingParams::default())
            .await
            .unwrap();
        assert!(c.tool_call.is_none());
        assert!(c.text.starts_with("[dummy reply]"));
    }
}
