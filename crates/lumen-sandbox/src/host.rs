//! 샌드박스와 공유되는 호스트 측 상태.

use std::sync::Arc;

use lumen_capability::PolicyEngine;
use lumen_core::{AgentId, Timestamp};
use parking_lot::Mutex;

/// 샌드박스 안에서 관찰된 단일 도구 호출 시도 기록.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCallRecord {
    /// 게스트가 호출한 도구 이름.
    pub tool: String,
    /// [`PolicyEngine`] 가 이 요청을 인가했는지 여부.
    pub authorised: bool,
}

/// 한 번의 실행 동안 wasmtime `Store` 가 소유하는 호스트 상태.
pub struct HostState {
    /// 신뢰된 정책 엔진.
    pub policy: Arc<PolicyEngine>,
    /// 이 샌드박스가 대표하는 에이전트의 식별자.
    pub agent: AgentId,
    /// capability 검사가 사용하는 "now". 단일 실행에서 시계 view 를 일관되게
    /// 유지하기 위해 캐시됩니다.
    pub now: Timestamp,
    /// 호스트 호출의 audit 로그 - 실행 후 호스트로 회수됨.
    pub audit: Mutex<Vec<String>>,
    /// 게스트에서 관찰된 도구 호출 시도들.
    pub tool_calls: Mutex<Vec<ToolCallRecord>>,
}

impl HostState {
    /// audit/log 버퍼가 빈 상태로 상태를 생성합니다.
    pub fn new(policy: Arc<PolicyEngine>, agent: AgentId) -> Self {
        Self {
            policy,
            agent,
            now: Timestamp::now(),
            audit: Mutex::new(Vec::new()),
            tool_calls: Mutex::new(Vec::new()),
        }
    }

    /// 사용자 정의 "now" 를 설정 - 결정론적 timestamp 가 필요한 테스트 용.
    #[must_use]
    pub fn with_now(mut self, now: Timestamp) -> Self {
        self.now = now;
        self
    }
}
