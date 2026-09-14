//! 다중 에이전트 tokio 슈퍼바이저.
//!
//! 아키텍처:
//!
//! ```text
//!     ┌────────────────────────────────────────────────┐
//!     │                  Orchestrator                  │
//!     │   agents: HashMap<AgentId, AgentSlot>          │
//!     │   (동기 등록부; 각 AgentSlot 은 tokio 태스크와   │
//!     │    에이전트별 inbox 채널을 소유)                  │
//!     └────────────────────────────────────────────────┘
//!                    │            ▲
//!         AgentCmd ──┘            └─ InterAgentMessage
//!         (step / shutdown)        (peer payload)
//!                    ▼
//!         ┌─────────────────────┐
//!         │     Agent Task      │   (에이전트당 tokio::spawn 한 개)
//!         │     ─ runtime.step()│
//!         └─────────────────────┘
//! ```
//!
//! 인터-에이전트 통신은 **별-라우팅** 입니다: 에이전트는 메모리를 공유하지
//! 않고, 서로의 `Sender` 를 직접 보유하지 않습니다 - 매 `send_to` 호출은
//! 오케스트레이터 등록부를 통해 수신자의 inbox 채널을 찾습니다. 등록부
//! 락은 `mpsc::Sender` 를 clone 하는 동안만 잡고, 그 뒤 전달은 경합 없이
//! 진행됩니다.
//!
//! Step 측 동시성: `AgentSender` 는 `Clone` 이므로 여러 tokio 태스크가 동시
//! `step` / `send_to` 를 발행할 수 있습니다. 에이전트 태스크는 명령을
//! 순차 처리해 결정론 step 계약을 유지합니다.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_agent::{AgentRuntime, StepResult};
use lumen_capability::{capability::Capability, Action, PolicyEngine};
use lumen_core::{AgentId, Error, Result, Timestamp};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// 에이전트별 기본 command 채널 깊이.
pub const DEFAULT_CMD_DEPTH: usize = 16;
/// 에이전트별 기본 인터-에이전트 inbox 깊이.
pub const DEFAULT_INBOX_DEPTH: usize = 64;

/// 단일 에이전트 태스크 스펙.
pub struct AgentSpec {
    /// 식별자 (런타임 내부 agent id 와 일치해야 함).
    pub agent_id: AgentId,
    /// 미리 빌드된 런타임 - spawn 된 태스크가 소유.
    pub runtime: Arc<AgentRuntime>,
}

/// 오케스트레이터를 통해 전달되는 인터-에이전트 메시지.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterAgentMessage {
    /// 전송 에이전트.
    pub from: AgentId,
    /// 불투명 postcard / JSON / 원시 byte 페이로드 - 호출자 정의.
    pub payload: Vec<u8>,
}

enum AgentCmd {
    Step {
        prompt: String,
        reply: oneshot::Sender<Result<StepResult>>,
    },
    Shutdown,
}

struct AgentSlot {
    cmd_tx: mpsc::Sender<AgentCmd>,
    inbox_tx: mpsc::Sender<InterAgentMessage>,
    join: JoinHandle<()>,
}

type AgentRegistry = Arc<Mutex<HashMap<AgentId, AgentSlot>>>;

/// 인터-에이전트 메시지 정책 강제기.
///
/// 오케스트레이터에 부착하면 [`AgentSender::send_to`] 가 거부되고, 모든
/// 송신은 [`AgentSender::send_to_authorized`] 로 capability 와 함께 와야
/// 합니다. 부착하지 않으면 정책 검증이 비활성화되어 v0.3 의 자유 전달
/// 모드로 동작합니다.
#[derive(Clone)]
pub struct MessagePolicy {
    engine: Arc<PolicyEngine>,
    now: Arc<dyn Fn() -> Timestamp + Send + Sync + 'static>,
}

impl MessagePolicy {
    /// 시스템 시간 기반 정책. 일반 배포용.
    pub fn new(engine: Arc<PolicyEngine>) -> Self {
        Self {
            engine,
            now: Arc::new(Timestamp::now),
        }
    }

    /// 결정론적 시간을 사용하는 정책 (테스트용).
    pub fn with_now<F>(engine: Arc<PolicyEngine>, now: F) -> Self
    where
        F: Fn() -> Timestamp + Send + Sync + 'static,
    {
        Self {
            engine,
            now: Arc::new(now),
        }
    }

    fn check(&self, cap: &Capability, sender: &AgentId, recipient: &AgentId) -> Result<()> {
        let action = Action::SendAgentMessage(recipient);
        self.engine.check(cap, sender, &action, (self.now)())
    }
}

impl std::fmt::Debug for MessagePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessagePolicy").finish_non_exhaustive()
    }
}

/// 에이전트 핸들의 clone 가능한 step / send-to 측면.
#[derive(Clone)]
pub struct AgentSender {
    agent_id: AgentId,
    cmd_tx: mpsc::Sender<AgentCmd>,
    registry: AgentRegistry,
    policy: Option<MessagePolicy>,
}

impl AgentSender {
    /// 대상 에이전트 식별자.
    pub fn agent_id(&self) -> AgentId {
        self.agent_id
    }

    /// step 요청을 제출하고 결과를 기다립니다.
    pub async fn step(&self, prompt: impl Into<String>) -> Result<StepResult> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(AgentCmd::Step {
                prompt: prompt.into(),
                reply: tx,
            })
            .await
            .map_err(|_| Error::Channel("agent task gone".into()))?;
        rx.await
            .map_err(|_| Error::Channel("agent reply dropped".into()))?
    }

    /// 다른 에이전트의 inbox 에 `payload` 를 전달합니다.
    ///
    /// 정책이 부착된 오케스트레이터에서는 거부됩니다 -
    /// [`AgentSender::send_to_authorized`] 를 사용해 capability 와 함께
    /// 송신하세요. 정책이 없으면 v0.3 호환 자유 전달 모드로 동작합니다.
    pub async fn send_to(&self, to: AgentId, payload: Vec<u8>) -> Result<()> {
        if self.policy.is_some() {
            return Err(Error::Policy(
                "send_to denied: orchestrator requires capability via send_to_authorized".into(),
            ));
        }
        self.deliver(to, payload).await
    }

    /// 정책이 부착된 오케스트레이터에서 capability 와 함께 송신합니다.
    ///
    /// 검증 단계:
    /// 1. capability 의 audience 가 송신자, resource 가 `AgentMessage(to)`,
    ///    서명이 신뢰된 issuer 의 키, 만료 미초과, nonce 미사용임을 확인.
    /// 2. 통과 시 수신자 inbox 로 전달.
    ///
    /// 정책이 부착되지 않은 오케스트레이터에서는 capability 를 무시하고
    /// 단순 전달합니다 - 호출 사이트가 명시적인 cap 사용 의도를 표현해
    /// 두면 추후 정책을 활성화할 때 코드 수정 없이 작동합니다.
    pub async fn send_to_authorized(
        &self,
        to: AgentId,
        payload: Vec<u8>,
        cap: &Capability,
    ) -> Result<()> {
        if let Some(policy) = &self.policy {
            policy.check(cap, &self.agent_id, &to)?;
        }
        self.deliver(to, payload).await
    }

    async fn deliver(&self, to: AgentId, payload: Vec<u8>) -> Result<()> {
        let target_tx = {
            let agents = self.registry.lock();
            let slot = agents
                .get(&to)
                .ok_or_else(|| Error::Channel(format!("no such agent: {to}")))?;
            slot.inbox_tx.clone()
        };
        target_tx
            .send(InterAgentMessage {
                from: self.agent_id,
                payload,
            })
            .await
            .map_err(|_| Error::Channel(format!("agent {to} inbox closed")))
    }
}

/// 에이전트 핸들의 수신 측면. 단일 consumer.
pub struct AgentInbox {
    agent_id: AgentId,
    inbox_rx: mpsc::Receiver<InterAgentMessage>,
}

impl AgentInbox {
    /// 소유 에이전트.
    pub fn agent_id(&self) -> AgentId {
        self.agent_id
    }

    /// 다음 인터-에이전트 메시지를 기다립니다.
    pub async fn recv(&mut self) -> Result<InterAgentMessage> {
        self.inbox_rx
            .recv()
            .await
            .ok_or_else(|| Error::Channel("inbox closed".into()))
    }

    /// 논블로킹 poll; 비어 있으면 `Ok(None)`.
    pub fn try_recv(&mut self) -> Result<Option<InterAgentMessage>> {
        match self.inbox_rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(Error::Channel("inbox closed".into()))
            }
        }
    }
}

/// [`Orchestrator::spawn`] 가 반환하는 sender + inbox 결합 핸들.
pub struct AgentHandle {
    sender: AgentSender,
    inbox: AgentInbox,
}

impl AgentHandle {
    /// 소유 에이전트.
    pub fn agent_id(&self) -> AgentId {
        self.sender.agent_id
    }

    /// Clone 가능한 sender 측면 (tokio 태스크 간 fan-out).
    pub fn sender(&self) -> AgentSender {
        self.sender.clone()
    }

    /// inbox 차용.
    pub fn inbox_mut(&mut self) -> &mut AgentInbox {
        &mut self.inbox
    }

    /// 소유권을 위해 두 절반으로 분리.
    pub fn split(self) -> (AgentSender, AgentInbox) {
        (self.sender, self.inbox)
    }

    /// 편의: [`AgentSender::step`] 으로 위임.
    pub async fn step(&self, prompt: impl Into<String>) -> Result<StepResult> {
        self.sender.step(prompt).await
    }

    /// 편의: [`AgentSender::send_to`] 으로 위임.
    pub async fn send_to(&self, to: AgentId, payload: Vec<u8>) -> Result<()> {
        self.sender.send_to(to, payload).await
    }

    /// 편의: [`AgentSender::send_to_authorized`] 로 위임.
    pub async fn send_to_authorized(
        &self,
        to: AgentId,
        payload: Vec<u8>,
        cap: &Capability,
    ) -> Result<()> {
        self.sender.send_to_authorized(to, payload, cap).await
    }

    /// 편의: [`AgentInbox::recv`] 로 위임.
    pub async fn recv_inbox(&mut self) -> Result<InterAgentMessage> {
        self.inbox.recv().await
    }

    /// 편의: [`AgentInbox::try_recv`] 로 위임.
    pub fn try_recv_inbox(&mut self) -> Result<Option<InterAgentMessage>> {
        self.inbox.try_recv()
    }
}

/// 다중 에이전트 슈퍼바이저.
#[derive(Clone, Default)]
pub struct Orchestrator {
    registry: AgentRegistry,
    policy: Option<MessagePolicy>,
}

impl Orchestrator {
    /// 정책이 없는 빈 오케스트레이터를 생성합니다 (v0.3 호환 모드).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 인터-에이전트 메시지 정책을 부착한 오케스트레이터를 생성합니다.
    ///
    /// 모든 송신은 [`AgentSender::send_to_authorized`] 로 capability 와
    /// 함께 와야 하며, plain [`AgentSender::send_to`] 는 [`Error::Policy`]
    /// 로 거부됩니다.
    #[must_use]
    pub fn with_policy(policy: MessagePolicy) -> Self {
        Self {
            registry: Arc::new(Mutex::new(HashMap::new())),
            policy: Some(policy),
        }
    }

    /// 현재 등록된 에이전트 수.
    pub fn agent_count(&self) -> usize {
        self.registry.lock().len()
    }

    /// 에이전트 태스크를 spawn 합니다.
    ///
    /// 같은 id 가 이미 등록되어 있으면 에러.
    pub fn spawn(&self, spec: AgentSpec) -> Result<AgentHandle> {
        let agent_id = spec.agent_id;
        let runtime = spec.runtime;

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<AgentCmd>(DEFAULT_CMD_DEPTH);
        let (inbox_tx, inbox_rx) = mpsc::channel::<InterAgentMessage>(DEFAULT_INBOX_DEPTH);

        let join = tokio::spawn(async move {
            tracing::info!(target: "lumen.orchestrator", %agent_id, "agent task started");
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    AgentCmd::Step { prompt, reply } => {
                        let result = runtime.step(&prompt).await;
                        // 응답 채널 drop 은 요청자가 더 이상 기다리지 않기로
                        // 결정한 것이므로 escalate 하지 않고 무시합니다.
                        let _ = reply.send(result);
                    }
                    AgentCmd::Shutdown => {
                        tracing::info!(target: "lumen.orchestrator", %agent_id, "shutdown");
                        break;
                    }
                }
            }
            tracing::info!(target: "lumen.orchestrator", %agent_id, "agent task ended");
        });

        let slot = AgentSlot {
            cmd_tx: cmd_tx.clone(),
            inbox_tx,
            join,
        };

        let mut agents = self.registry.lock();
        if agents.contains_key(&agent_id) {
            return Err(Error::Invalid(format!(
                "agent already registered: {agent_id}"
            )));
        }
        agents.insert(agent_id, slot);
        drop(agents);

        Ok(AgentHandle {
            sender: AgentSender {
                agent_id,
                cmd_tx,
                registry: self.registry.clone(),
                policy: self.policy.clone(),
            },
            inbox: AgentInbox { agent_id, inbox_rx },
        })
    }

    /// 단일 에이전트 종료. id 가 등록되지 않았다면 no-op.
    pub async fn shutdown(&self, agent_id: AgentId) -> Result<()> {
        let slot = self.registry.lock().remove(&agent_id);
        if let Some(slot) = slot {
            let AgentSlot { cmd_tx, join, .. } = slot;
            let _ = cmd_tx.send(AgentCmd::Shutdown).await;
            join.await
                .map_err(|e| Error::Channel(format!("agent {agent_id} task panicked: {e}")))?;
        }
        Ok(())
    }

    /// 모든 에이전트를 종료하고 태스크가 종결될 때까지 기다립니다.
    pub async fn shutdown_all(&self) -> Result<()> {
        let slots: Vec<_> = self.registry.lock().drain().collect();
        let mut first_err: Option<Error> = None;
        for (id, slot) in slots {
            let AgentSlot { cmd_tx, join, .. } = slot;
            let _ = cmd_tx.send(AgentCmd::Shutdown).await;
            if let Err(e) = join.await {
                let err = Error::Channel(format!("agent {id} task panicked: {e}"));
                if first_err.is_none() {
                    first_err = Some(err);
                } else {
                    tracing::warn!(target: "lumen.orchestrator", %id, "task panic suppressed");
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use lumen_agent::tool::{EchoTool, Tool};
    use lumen_capability::{
        capability::{Capability, CapabilityBody},
        PolicyEngine, Resource,
    };
    use lumen_core::rng::OsRng;
    use lumen_core::{Blake3Hash, CapabilityId, SigningKey, Timestamp, ToolId};
    use lumen_inference::DummyEngine;
    use lumen_zkml::mock::MockVk;

    fn build_runtime(nonce: [u8; 16]) -> (AgentId, Arc<AgentRuntime>) {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let agent = AgentId::random(&mut OsRng);
        let policy = Arc::new(PolicyEngine::new(vec![vk]));
        let echo = ToolId::new("echo").unwrap();
        let cap = Capability::sign(
            CapabilityBody {
                id: CapabilityId::random(&mut OsRng),
                audience: agent,
                resource: Resource::Tool(echo.clone()),
                nonce,
                expires_at: Timestamp::FOREVER,
                issuer: agent,
            },
            &sk,
        )
        .unwrap();
        let rt = AgentRuntime::builder(agent, policy)
            .inference(Arc::new(DummyEngine::new()))
            .tool(Tool {
                id: echo.clone(),
                schema: serde_json::json!({}),
                handler: Arc::new(EchoTool),
            })
            .unwrap()
            .capability(echo, cap)
            .proving_vk(MockVk {
                circuit_id: "lumen.routing.v1".into(),
            })
            .policy_hash(Blake3Hash::of(b"policy"))
            .build()
            .unwrap();
        (agent, Arc::new(rt))
    }

    #[tokio::test]
    async fn single_agent_spawn_step_shutdown() {
        let orch = Orchestrator::new();
        let (agent_id, runtime) = build_runtime([1; 16]);
        let handle = orch.spawn(AgentSpec { agent_id, runtime }).unwrap();
        let res = handle.step("echo greetings").await.unwrap();
        assert!(res.completion.tool_call.is_some());
        assert_eq!(orch.agent_count(), 1);
        orch.shutdown_all().await.unwrap();
        assert_eq!(orch.agent_count(), 0);
    }

    #[tokio::test]
    async fn multiple_agents_run_in_parallel() {
        let orch = Orchestrator::new();
        let mut handles = Vec::new();
        for i in 0u8..3 {
            let mut nonce = [0u8; 16];
            nonce[0] = i;
            let (agent_id, runtime) = build_runtime(nonce);
            handles.push(orch.spawn(AgentSpec { agent_id, runtime }).unwrap());
        }
        assert_eq!(orch.agent_count(), 3);

        // Issue steps on all three concurrently.
        let senders: Vec<_> = handles.iter().map(AgentHandle::sender).collect();
        let mut joins = Vec::new();
        for (i, sender) in senders.into_iter().enumerate() {
            joins.push(tokio::spawn(async move {
                sender.step(format!("echo agent-{i}")).await
            }));
        }
        for j in joins {
            let res = j.await.unwrap().unwrap();
            assert!(res.completion.tool_call.is_some());
        }

        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn duplicate_agent_id_rejected() {
        let orch = Orchestrator::new();
        let (agent_id, runtime) = build_runtime([7; 16]);
        let _h1 = orch
            .spawn(AgentSpec {
                agent_id,
                runtime: runtime.clone(),
            })
            .unwrap();
        let err = match orch.spawn(AgentSpec { agent_id, runtime }) {
            Ok(_) => panic!("duplicate agent should have been rejected"),
            Err(e) => e,
        };
        assert!(matches!(err, Error::Invalid(_)));
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn inter_agent_send_and_recv() {
        let orch = Orchestrator::new();
        let (a_id, a_rt) = build_runtime([1; 16]);
        let (b_id, b_rt) = build_runtime([2; 16]);
        let h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let mut h_b = orch
            .spawn(AgentSpec {
                agent_id: b_id,
                runtime: b_rt,
            })
            .unwrap();

        h_a.send_to(b_id, b"hello-from-a".to_vec()).await.unwrap();
        let msg = h_b.recv_inbox().await.unwrap();
        assert_eq!(msg.from, a_id);
        assert_eq!(msg.payload, b"hello-from-a");

        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn send_to_unknown_agent_errors() {
        let orch = Orchestrator::new();
        let (a_id, a_rt) = build_runtime([1; 16]);
        let h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let bogus = AgentId::from_bytes([0xFF; 16]);
        let err = h_a.send_to(bogus, b"x".to_vec()).await.unwrap_err();
        assert!(matches!(err, Error::Channel(_)));
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn try_recv_returns_none_when_empty() {
        let orch = Orchestrator::new();
        let (a_id, a_rt) = build_runtime([1; 16]);
        let mut h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        assert!(h_a.try_recv_inbox().unwrap().is_none());
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn send_to_denied_when_policy_attached() {
        let sk = SigningKey::generate(&mut OsRng);
        let policy = Arc::new(PolicyEngine::new(vec![sk.verifying_key()]));
        let orch = Orchestrator::with_policy(MessagePolicy::with_now(policy, || {
            Timestamp::from_millis(0)
        }));
        let (a_id, a_rt) = build_runtime([1; 16]);
        let (b_id, b_rt) = build_runtime([2; 16]);
        let h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let _h_b = orch
            .spawn(AgentSpec {
                agent_id: b_id,
                runtime: b_rt,
            })
            .unwrap();
        // 정책 부착 시 plain send_to 는 거부.
        let err = h_a.send_to(b_id, b"hi".to_vec()).await.unwrap_err();
        assert!(matches!(err, Error::Policy(_)));
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn send_to_authorized_with_valid_capability_succeeds() {
        let sk = SigningKey::generate(&mut OsRng);
        let policy_engine = Arc::new(PolicyEngine::new(vec![sk.verifying_key()]));
        let orch = Orchestrator::with_policy(MessagePolicy::with_now(policy_engine, || {
            Timestamp::from_millis(0)
        }));
        let (a_id, a_rt) = build_runtime([1; 16]);
        let (b_id, b_rt) = build_runtime([2; 16]);
        let h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let mut h_b = orch
            .spawn(AgentSpec {
                agent_id: b_id,
                runtime: b_rt,
            })
            .unwrap();

        // a → b 송신을 허가하는 capability.
        let cap = Capability::sign(
            CapabilityBody {
                id: CapabilityId::random(&mut OsRng),
                audience: a_id,
                resource: Resource::AgentMessage(b_id),
                nonce: [42u8; 16],
                expires_at: Timestamp::FOREVER,
                issuer: a_id,
            },
            &sk,
        )
        .unwrap();

        h_a.send_to_authorized(b_id, b"hi-with-cap".to_vec(), &cap)
            .await
            .unwrap();
        let msg = h_b.recv_inbox().await.unwrap();
        assert_eq!(msg.from, a_id);
        assert_eq!(msg.payload, b"hi-with-cap");
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn send_to_authorized_wrong_recipient_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let policy_engine = Arc::new(PolicyEngine::new(vec![sk.verifying_key()]));
        let orch = Orchestrator::with_policy(MessagePolicy::with_now(policy_engine, || {
            Timestamp::from_millis(0)
        }));
        let (a_id, a_rt) = build_runtime([1; 16]);
        let (b_id, b_rt) = build_runtime([2; 16]);
        let (c_id, c_rt) = build_runtime([3; 16]);
        let h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let _h_b = orch
            .spawn(AgentSpec {
                agent_id: b_id,
                runtime: b_rt,
            })
            .unwrap();
        let _h_c = orch
            .spawn(AgentSpec {
                agent_id: c_id,
                runtime: c_rt,
            })
            .unwrap();

        // a 는 b 에게 보낼 권한만 있음 - c 로 보내려 하면 거부.
        let cap_to_b = Capability::sign(
            CapabilityBody {
                id: CapabilityId::random(&mut OsRng),
                audience: a_id,
                resource: Resource::AgentMessage(b_id),
                nonce: [99u8; 16],
                expires_at: Timestamp::FOREVER,
                issuer: a_id,
            },
            &sk,
        )
        .unwrap();
        let err = h_a
            .send_to_authorized(c_id, b"hi-to-c".to_vec(), &cap_to_b)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn send_to_authorized_capability_replay_rejected() {
        let sk = SigningKey::generate(&mut OsRng);
        let policy_engine = Arc::new(PolicyEngine::new(vec![sk.verifying_key()]));
        let orch = Orchestrator::with_policy(MessagePolicy::with_now(policy_engine, || {
            Timestamp::from_millis(0)
        }));
        let (a_id, a_rt) = build_runtime([1; 16]);
        let (b_id, b_rt) = build_runtime([2; 16]);
        let h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let _h_b = orch
            .spawn(AgentSpec {
                agent_id: b_id,
                runtime: b_rt,
            })
            .unwrap();
        let cap = Capability::sign(
            CapabilityBody {
                id: CapabilityId::random(&mut OsRng),
                audience: a_id,
                resource: Resource::AgentMessage(b_id),
                nonce: [11u8; 16],
                expires_at: Timestamp::FOREVER,
                issuer: a_id,
            },
            &sk,
        )
        .unwrap();
        h_a.send_to_authorized(b_id, b"once".to_vec(), &cap)
            .await
            .unwrap();
        // 동일 nonce 의 cap 재사용은 거부.
        let err = h_a
            .send_to_authorized(b_id, b"twice".to_vec(), &cap)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Capability(_)));
        orch.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_one_keeps_others() {
        let orch = Orchestrator::new();
        let (a_id, a_rt) = build_runtime([1; 16]);
        let (b_id, b_rt) = build_runtime([2; 16]);
        let _h_a = orch
            .spawn(AgentSpec {
                agent_id: a_id,
                runtime: a_rt,
            })
            .unwrap();
        let h_b = orch
            .spawn(AgentSpec {
                agent_id: b_id,
                runtime: b_rt,
            })
            .unwrap();
        assert_eq!(orch.agent_count(), 2);

        orch.shutdown(a_id).await.unwrap();
        assert_eq!(orch.agent_count(), 1);

        // Surviving agent still works.
        let res = h_b.step("echo still-alive").await.unwrap();
        assert!(res.completion.tool_call.is_some());

        orch.shutdown_all().await.unwrap();
    }
}
