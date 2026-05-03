//! 정책 강제: capability 검증 + 리플레이 방지 + audit.

use std::collections::BTreeMap;
use std::path::Path;

use lumen_core::{AgentId, Error, Result, Timestamp, ToolId, VerifyingKey};

use parking_lot::Mutex;

use crate::audit;
use crate::capability::Capability;
use crate::resource::{HostPattern, PathPattern, Resource};

/// 에이전트가 권한을 요청하는 대상.
#[derive(Clone, Debug)]
pub enum Action<'a> {
    /// `path` 로부터 읽기.
    FsRead(&'a Path),
    /// `path` 로 쓰기.
    FsWrite(&'a Path),
    /// `host` 로 연결.
    Net(&'a str),
    /// `tool` 호출.
    CallTool(&'a ToolId),
    /// `n` 만큼의 추론 토큰 사용.
    SpendInferenceTokens(u32),
    /// ZK 증명 요청.
    ZkProofRequest,
    /// 다른 에이전트에게 인터-에이전트 메시지 송신.
    SendAgentMessage(&'a AgentId),
}

/// 동시에 추적되는 nonce 의 최대 개수. 공격자가 capability 를 무한 발행해
/// 메모리를 고갈시키는 것을 막습니다.
const MAX_NONCES: usize = 65_536;

/// 서명된 capability 를 검증하고 리플레이/만료/audience 규칙을 강제합니다.
pub struct PolicyEngine {
    trusted: Vec<VerifyingKey>,
    seen_nonces: Mutex<BTreeMap<[u8; 16], Timestamp>>,
}

impl PolicyEngine {
    /// 주어진 issuer key 들을 신뢰하는 엔진을 생성합니다.
    pub fn new(trusted_issuers: Vec<VerifyingKey>) -> Self {
        Self {
            trusted: trusted_issuers,
            seen_nonces: Mutex::new(BTreeMap::new()),
        }
    }

    /// 신뢰된 issuer key 추가.
    pub fn add_issuer(&mut self, key: VerifyingKey) {
        self.trusted.push(key);
    }

    /// Capability 를 검증하고 액션을 인가합니다.
    ///
    /// 성공 시 capability 의 nonce 를 기록하므로 리플레이 시도는
    /// [`Error::Capability`] 를 반환합니다.
    pub fn check(
        &self,
        cap: &Capability,
        agent: &AgentId,
        action: &Action<'_>,
        now: Timestamp,
    ) -> Result<()> {
        // 1. Trust anchor: 서명이 신뢰된 키 중 적어도 하나로 검증되어야 합니다.
        let mut sig_ok = false;
        for key in &self.trusted {
            if cap.verify_signature(key).is_ok() {
                sig_ok = true;
                break;
            }
        }
        if !sig_ok {
            audit::deny(Some(cap), action, "signature");
            return Err(Error::Capability("signature did not verify".into()));
        }

        // 2. Audience: capability 가 요청 에이전트로 발행되어야 합니다.
        if cap.body.audience != *agent {
            audit::deny(Some(cap), action, "audience");
            return Err(Error::Capability("wrong audience".into()));
        }

        // 3. 만료.
        if now >= cap.body.expires_at {
            audit::deny(Some(cap), action, "expired");
            return Err(Error::Capability("expired".into()));
        }

        // 4. 리소스 일치.
        if !resource_matches(&cap.body.resource, action) {
            audit::deny(Some(cap), action, "resource_mismatch");
            return Err(Error::Capability("resource mismatch".into()));
        }

        // 5. 리플레이 방지 - 거부되었어야 할 capability 의 nonce 를 소진하지
        //    않도록 가장 마지막에 수행합니다.
        {
            let mut seen = self.seen_nonces.lock();
            // 만료된 항목을 lazy 하게 정리.
            seen.retain(|_, exp| *exp > now);
            if seen.len() >= MAX_NONCES {
                audit::deny(Some(cap), action, "nonce_table_full");
                return Err(Error::Capability("nonce table full".into()));
            }
            if seen.insert(cap.body.nonce, cap.body.expires_at).is_some() {
                audit::deny(Some(cap), action, "replay");
                return Err(Error::Capability("nonce replay".into()));
            }
        }

        audit::allow(cap, action);
        Ok(())
    }
}

fn resource_matches(resource: &Resource, action: &Action<'_>) -> bool {
    match (resource, action) {
        (Resource::FsRead(pat), Action::FsRead(path)) => pat.matches(path),
        (Resource::FsWrite(pat), Action::FsWrite(path)) => pat.matches(path),
        (Resource::Net(pat), Action::Net(host)) => pat.matches(host),
        (Resource::Tool(allowed), Action::CallTool(want)) => *want == allowed,
        (Resource::InferenceTokens { max }, Action::SpendInferenceTokens(n)) => n <= max,
        (Resource::ZkProofRequest, Action::ZkProofRequest) => true,
        (Resource::AgentMessage(allowed), Action::SendAgentMessage(want)) => *want == allowed,
        _ => false,
    }
}

// 공개 API 가 `globset` 타입을 사용함을 보장.
#[allow(dead_code)]
fn _ensure_pattern_types_compile(_: &PathPattern, _: &HostPattern) {}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::{AgentId, CapabilityId, SigningKey};
    use rand::rngs::OsRng;

    use crate::capability::{Capability, CapabilityBody};

    fn issue_tool_cap(
        sk: &SigningKey,
        audience: AgentId,
        issuer: AgentId,
        tool: &str,
        nonce: [u8; 16],
        expires_at: Timestamp,
    ) -> Capability {
        let body = CapabilityBody {
            id: CapabilityId::random(&mut OsRng),
            audience,
            resource: Resource::Tool(ToolId::new(tool).unwrap()),
            nonce,
            expires_at,
            issuer,
        };
        Capability::sign(body, sk).unwrap()
    }

    #[test]
    fn allow_then_replay_fails() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let agent = AgentId::random(&mut OsRng);
        let issuer = AgentId::random(&mut OsRng);
        let engine = PolicyEngine::new(vec![vk]);
        let now = Timestamp::from_millis(1000);
        let cap = issue_tool_cap(&sk, agent, issuer, "echo", [9; 16], Timestamp::FOREVER);
        let tool = ToolId::new("echo").unwrap();

        engine
            .check(&cap, &agent, &Action::CallTool(&tool), now)
            .expect("first use ok");
        // 동일 nonce 의 리플레이는 실패해야 합니다.
        let err = engine
            .check(&cap, &agent, &Action::CallTool(&tool), now)
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
    }

    #[test]
    fn expired_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let agent = AgentId::random(&mut OsRng);
        let engine = PolicyEngine::new(vec![vk]);
        let cap = issue_tool_cap(
            &sk,
            agent,
            agent,
            "echo",
            [1; 16],
            Timestamp::from_millis(500),
        );
        let tool = ToolId::new("echo").unwrap();
        let err = engine
            .check(
                &cap,
                &agent,
                &Action::CallTool(&tool),
                Timestamp::from_millis(500),
            )
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
    }

    #[test]
    fn wrong_audience_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let agent = AgentId::random(&mut OsRng);
        let other = AgentId::random(&mut OsRng);
        let engine = PolicyEngine::new(vec![vk]);
        let cap = issue_tool_cap(&sk, other, other, "echo", [2; 16], Timestamp::FOREVER);
        let tool = ToolId::new("echo").unwrap();
        let err = engine
            .check(
                &cap,
                &agent,
                &Action::CallTool(&tool),
                Timestamp::from_millis(0),
            )
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
    }

    #[test]
    fn untrusted_issuer_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let other_sk = SigningKey::generate(&mut OsRng);
        let agent = AgentId::random(&mut OsRng);
        let engine = PolicyEngine::new(vec![other_sk.verifying_key()]);
        let cap = issue_tool_cap(&sk, agent, agent, "echo", [3; 16], Timestamp::FOREVER);
        let tool = ToolId::new("echo").unwrap();
        let err = engine
            .check(
                &cap,
                &agent,
                &Action::CallTool(&tool),
                Timestamp::from_millis(0),
            )
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
    }

    #[test]
    fn resource_mismatch_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let agent = AgentId::random(&mut OsRng);
        let engine = PolicyEngine::new(vec![vk]);
        let cap = issue_tool_cap(&sk, agent, agent, "echo", [4; 16], Timestamp::FOREVER);
        let other = ToolId::new("rm").unwrap();
        let err = engine
            .check(
                &cap,
                &agent,
                &Action::CallTool(&other),
                Timestamp::from_millis(0),
            )
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
    }
}
