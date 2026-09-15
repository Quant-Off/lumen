//! 자유 텍스트 completion 에서 구조화된 [`ToolCall`] 을 추출하는 규약.
//!
//! LLM 백엔드 대부분은 도구 호출을 텍스트로만 표현합니다. Lumen 은 백엔드
//! 독립적인 단일 규약을 둡니다. 모델 출력이 다음 JSON 객체를 담고 있으면
//! 도구 호출로 해석합니다.
//!
//! ```json
//! {"tool": "<tool id>", "args": { ... }}
//! ```
//!
//! [`parse_tool_call`] 은 (1) 전체 텍스트, (2) ```` ```json ```` 코드 펜스
//! 내부, (3) 첫 `{` 부터 마지막 `}` 까지의 부분 문자열 순으로 시도합니다.
//! 탐색은 결정론적이며 정규식 / 백트래킹이 없습니다.
//!
//! 문법 제약을 지원하는 백엔드 (llama.cpp GBNF) 에는 [`ToolCallGrammar`] 가
//! 생성한 문법을 전달해 모델이 규약 밖의 출력을 내는 것을 원천 차단할 수
//! 있습니다. 문법이 도구 ID 집합을 열거하므로 등록되지 않은 도구 이름은
//! 토큰 수준에서 생성 불가능합니다.

use lumen_core::ToolId;
use serde::Deserialize;

use crate::ToolCall;

/// 도구 호출 텍스트 스캔 상한 (바이트). 초과분은 무시합니다.
const MAX_SCAN_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct Wire {
    tool: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

/// 텍스트에서 `{"tool": ..., "args": {...}}` 객체를 찾아 [`ToolCall`] 로
/// 변환합니다. 규약에 맞는 객체가 없으면 `None`.
///
/// `args` 가 생략되면 빈 객체 `{}` 로 취급합니다. `args` 가 객체가 아니거나
/// `tool` 이 유효한 [`ToolId`] 가 아니면 `None` 입니다 (에러가 아닌 "도구
/// 호출 아님" 으로 처리해 자유 텍스트 응답 경로로 넘깁니다).
pub fn parse_tool_call(text: &str) -> Option<ToolCall> {
    let text = truncate_at_char_boundary(text, MAX_SCAN_BYTES);
    let trimmed = text.trim();
    if let Some(call) = try_parse(trimmed) {
        return Some(call);
    }
    if let Some(inner) = fenced_block(trimmed) {
        if let Some(call) = try_parse(inner.trim()) {
            return Some(call);
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    try_parse(&trimmed[start..=end])
}

fn try_parse(candidate: &str) -> Option<ToolCall> {
    if !candidate.starts_with('{') {
        return None;
    }
    let wire: Wire = serde_json::from_str(candidate).ok()?;
    let id = ToolId::new(wire.tool).ok()?;
    let args = wire.args.unwrap_or_else(|| serde_json::json!({}));
    if !args.is_object() {
        return None;
    }
    // `serde_json::Value` 는 preserve_order 미사용 시 BTreeMap 이라 직렬화가
    // 키 정렬 순으로 결정론적입니다. args_hash 가 witness 에 들어가므로
    // 이 정규화가 중요합니다.
    let args_json = serde_json::to_string(&args).ok()?;
    Some(ToolCall { id, args_json })
}

fn fenced_block(text: &str) -> Option<&str> {
    let open = text.find("```")?;
    let after = &text[open + 3..];
    let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
    let body = &after[body_start..];
    let close = body.find("```")?;
    Some(&body[..close])
}

fn truncate_at_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 도구 호출 규약을 강제하는 GBNF 문법 생성기.
///
/// llama.cpp 의 GBNF 문법 (`grammar` 필드) 으로 렌더링됩니다. `root` 는
/// 정확히 `{"tool": <등록된 id 중 하나>, "args": <JSON 객체>}` 만 허용
/// 합니다. 도구 목록이 비어 있으면 [`ToolCallGrammar::gbnf`] 는 `None` 을
/// 반환합니다 (제약 없는 자유 텍스트).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolCallGrammar {
    tools: Vec<ToolId>,
}

impl ToolCallGrammar {
    /// 도구 ID 집합으로 생성. 중복은 제거되고 정렬됩니다.
    pub fn new<I: IntoIterator<Item = ToolId>>(tools: I) -> Self {
        let mut tools: Vec<ToolId> = tools.into_iter().collect();
        tools.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        tools.dedup_by(|a, b| a.as_str() == b.as_str());
        Self { tools }
    }

    /// 등록된 도구 ID.
    pub fn tools(&self) -> &[ToolId] {
        &self.tools
    }

    /// GBNF 문법 텍스트. 도구가 없으면 `None`.
    pub fn gbnf(&self) -> Option<String> {
        if self.tools.is_empty() {
            return None;
        }
        // ToolId 는 [A-Za-z0-9_.-]+ 로 검증되므로 GBNF 리터럴 이스케이프가
        // 필요 없습니다.
        let alts = self
            .tools
            .iter()
            .map(|t| format!("\"\\\"{}\\\"\"", t.as_str()))
            .collect::<Vec<_>>()
            .join(" | ");
        Some(format!(
            "root ::= \"{{\" ws \"\\\"tool\\\"\" ws \":\" ws tool ws \",\" ws \"\\\"args\\\"\" ws \":\" ws object ws \"}}\"\n\
             tool ::= {alts}\n\
             object ::= \"{{\" ws ( string ws \":\" ws value ( ws \",\" ws string ws \":\" ws value )* )? ws \"}}\"\n\
             array ::= \"[\" ws ( value ( ws \",\" ws value )* )? ws \"]\"\n\
             value ::= object | array | string | number | \"true\" | \"false\" | \"null\"\n\
             string ::= \"\\\"\" ( [^\"\\\\\\x7F\\x00-\\x1F] | \"\\\\\" ( [\"\\\\/bfnrt] | \"u\" hex hex hex hex ) )* \"\\\"\"\n\
             hex ::= [0-9a-fA-F]\n\
             number ::= \"-\"? ( \"0\" | [1-9] [0-9]* ) ( \".\" [0-9]+ )? ( [eE] [-+]? [0-9]+ )?\n\
             ws ::= [ \\t\\n]*\n"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_object() {
        let c = parse_tool_call(r#"{"tool":"echo","args":{"text":"hi"}}"#).unwrap();
        assert_eq!(c.id.as_str(), "echo");
        assert_eq!(c.args_json, r#"{"text":"hi"}"#);
    }

    #[test]
    fn fenced_and_surrounded() {
        let text = "Sure, calling the tool:\n```json\n{\"tool\": \"add\", \"args\": {\"b\": 2, \"a\": 1}}\n```\nDone.";
        let c = parse_tool_call(text).unwrap();
        assert_eq!(c.id.as_str(), "add");
        // 키 정렬로 정규화됨.
        assert_eq!(c.args_json, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn embedded_without_fence() {
        let c = parse_tool_call("thinking... {\"tool\":\"echo\",\"args\":{}} ok").unwrap();
        assert_eq!(c.id.as_str(), "echo");
        assert_eq!(c.args_json, "{}");
    }

    #[test]
    fn missing_args_defaults_to_empty_object() {
        let c = parse_tool_call(r#"{"tool":"echo"}"#).unwrap();
        assert_eq!(c.args_json, "{}");
    }

    #[test]
    fn rejects_non_object_args_and_bad_ids() {
        assert!(parse_tool_call(r#"{"tool":"echo","args":[1]}"#).is_none());
        assert!(parse_tool_call(r#"{"tool":"bad id!","args":{}}"#).is_none());
        assert!(parse_tool_call(r#"{"tool":"","args":{}}"#).is_none());
        assert!(parse_tool_call("no json here").is_none());
        assert!(parse_tool_call("").is_none());
    }

    #[test]
    fn deterministic_across_calls() {
        let t = r#"{"tool":"echo","args":{"z":1,"a":[1,2,{"y":true}]}}"#;
        assert_eq!(parse_tool_call(t), parse_tool_call(t));
    }

    #[test]
    fn grammar_lists_tools_sorted_and_deduped() {
        let g = ToolCallGrammar::new([
            ToolId::new("echo").unwrap(),
            ToolId::new("add").unwrap(),
            ToolId::new("echo").unwrap(),
        ]);
        assert_eq!(g.tools().len(), 2);
        let text = g.gbnf().unwrap();
        assert!(text.contains("tool ::= \"\\\"add\\\"\" | \"\\\"echo\\\"\""));
        assert!(text.starts_with("root ::= "));
        assert!(ToolCallGrammar::new([]).gbnf().is_none());
    }
}
