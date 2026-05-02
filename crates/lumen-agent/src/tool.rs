//! 도구 등록부 - 에이전트가 dispatch 할 수 있는 호스트 측 함수.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use lumen_core::{Error, Result, ToolId};
use serde_json::Value as Json;

/// 도구 호출을 실행하는 trait.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// JSON 인코딩된 인자에 대해 도구를 실행하고 JSON 인코딩된 출력을
    /// 반환합니다.
    async fn call(&self, args_json: &str) -> Result<String>;
}

/// 등록부의 항목 한 개.
pub struct Tool {
    /// 도구 식별자.
    pub id: ToolId,
    /// 인자를 기술하는 JSON 스키마. v0 에서는 자유 형식.
    pub schema: Json,
    /// 구현체.
    pub handler: Arc<dyn ToolHandler>,
}

/// 결정론적 등록부 (BTreeMap 이라 순회 순서가 고정).
#[derive(Default)]
pub struct ToolRegistry {
    inner: BTreeMap<ToolId, Tool>,
}

impl ToolRegistry {
    /// 빈 등록부.
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// 도구 등록. 같은 id 가 두 번 등록되면 에러를 반환합니다.
    pub fn register(&mut self, tool: Tool) -> Result<()> {
        if self.inner.contains_key(&tool.id) {
            return Err(Error::Invalid(format!(
                "tool id already registered: {}",
                tool.id
            )));
        }
        self.inner.insert(tool.id.clone(), tool);
        Ok(())
    }

    /// id 로 도구 조회.
    pub fn get(&self, id: &ToolId) -> Option<&Tool> {
        self.inner.get(id)
    }

    /// 등록된 도구 개수.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// 등록부가 비어 있는지 여부.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 결정론적 순서의 iterator.
    pub fn iter(&self) -> impl Iterator<Item = (&ToolId, &Tool)> {
        self.inner.iter()
    }
}

/// `EchoTool` - `args.text` 를 그대로 돌려주는 미니멀 핸들러. 테스트, 예제,
/// v0 데모 CLI 에 유용합니다.
#[derive(Default, Clone)]
pub struct EchoTool;

#[async_trait]
impl ToolHandler for EchoTool {
    async fn call(&self, args_json: &str) -> Result<String> {
        let v: Json =
            serde_json::from_str(args_json).map_err(|e| Error::Invalid(format!("args: {e}")))?;
        let text = v
            .get("text")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::Invalid("expected {\"text\": \"...\"}".into()))?;
        Ok(serde_json::json!({ "echoed": text }).to_string())
    }
}

/// `AddTool` - 두 정수의 합. 데모 전용.
#[derive(Default, Clone)]
pub struct AddTool;

#[async_trait]
impl ToolHandler for AddTool {
    async fn call(&self, args_json: &str) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            a: i64,
            b: i64,
        }
        let a: Args =
            serde_json::from_str(args_json).map_err(|e| Error::Invalid(format!("args: {e}")))?;
        let sum = a.a.saturating_add(a.b);
        Ok(serde_json::json!({ "sum": sum }).to_string())
    }
}
