//! `AgentRuntime` - defense -> infer -> policy -> tool -> prove 를 오케스트레이션합니다.

use std::collections::BTreeMap;
use std::sync::Arc;

use lumen_capability::{Action, Capability, PolicyEngine};
use lumen_core::{AgentId, Blake3Hash, Error, Result, Timestamp, ToolId};
use lumen_defense::{DefenseEngine, Verdict};
use lumen_inference::{Completion, InferenceEngine, SamplingParams};
use lumen_zkml::mock::{verify_with_witness, MockProof, MockVk};
use lumen_zkml::ProvingSystem;
use serde::{Deserialize, Serialize};

use crate::route::{RoutingDecision, RoutingPublicInputs, RoutingWitness};
use crate::tool::ToolRegistry;

/// [`AgentRuntime::step`] 의 반환값.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepResult {
    /// 추론 엔진의 completion.
    pub completion: Completion,
    /// 실행된 도구의 JSON 출력 (있다면).
    pub tool_output: Option<String>,
    /// 라우팅 결정 (public + witness).
    pub routing: RoutingDecision,
    /// Mock-prover commitment.
    pub proof: MockProof,
    /// 자기 검증 verdict (mock prover 에서는 항상 `CommitmentOnly` 또는
    /// `Invalid`; 절대 `ZkVerified` 가 아님).
    pub verification: lumen_zkml::Verification,
    /// 입력 프롬프트에 대한 defense verdict.
    pub defense_verdict: Verdict,
}

/// 런타임이 생성 시점에 필요로 하는 입력들.
pub struct AgentRuntime {
    agent: AgentId,
    policy: Arc<PolicyEngine>,
    /// 도구 id 키의 capability; 런타임은 호출당 하나를 사용합니다.
    caps_by_tool: BTreeMap<ToolId, Capability>,
    defense: DefenseEngine,
    inference: Arc<dyn InferenceEngine>,
    tools: ToolRegistry,
    proving_vk: MockVk,
    policy_hash: Blake3Hash,
    sampling: SamplingParams,
}

/// [`AgentRuntime`] 빌더.
pub struct AgentRuntimeBuilder {
    agent: AgentId,
    policy: Arc<PolicyEngine>,
    caps_by_tool: BTreeMap<ToolId, Capability>,
    defense: Option<DefenseEngine>,
    inference: Option<Arc<dyn InferenceEngine>>,
    tools: ToolRegistry,
    proving_vk: Option<MockVk>,
    policy_hash: Option<Blake3Hash>,
    sampling: SamplingParams,
}

impl AgentRuntime {
    /// 빌더 시작.
    pub fn builder(agent: AgentId, policy: Arc<PolicyEngine>) -> AgentRuntimeBuilder {
        AgentRuntimeBuilder {
            agent,
            policy,
            caps_by_tool: BTreeMap::new(),
            defense: None,
            inference: None,
            tools: ToolRegistry::new(),
            proving_vk: None,
            policy_hash: None,
            sampling: SamplingParams::default(),
        }
    }

    /// 한 번의 에이전트 step 실행.
    pub async fn step(&self, prompt: &str) -> Result<StepResult> {
        // 1. Defense.
        let defense_verdict = self.defense.analyze(prompt);
        if let Verdict::Block(reason) = &defense_verdict {
            tracing::warn!(target: "lumen.agent", ?reason, "blocked");
            return Err(Error::Defense(format!("{reason:?}")));
        }

        // 2. Inference.
        let completion = self.inference.complete(prompt, &self.sampling).await?;

        // 3. Policy + tool.
        let mut tool_output: Option<String> = None;
        let mut chosen_tool: Option<ToolId> = None;
        let mut args_hash: Option<Blake3Hash> = None;
        if let Some(call) = &completion.tool_call {
            let cap = self
                .caps_by_tool
                .get(&call.id)
                .ok_or_else(|| Error::Capability(format!("no capability for tool {}", call.id)))?;
            let now = Timestamp::now();
            self.policy
                .check(cap, &self.agent, &Action::CallTool(&call.id), now)?;

            let tool = self
                .tools
                .get(&call.id)
                .ok_or_else(|| Error::Invalid(format!("tool not registered: {}", call.id)))?;
            let output = tool.handler.call(&call.args_json).await?;
            args_hash = Some(Blake3Hash::of(call.args_json.as_bytes()));
            tool_output = Some(output);
            chosen_tool = Some(call.id.clone());
        }

        // 4. 라우팅 결정 빌드 후 prove.
        let public = RoutingPublicInputs {
            prompt_hash: Blake3Hash::of(prompt.as_bytes()),
            policy_hash: self.policy_hash,
            tool: chosen_tool,
        };
        let witness = RoutingWitness {
            args_hash,
            defense_corpus: self.defense.corpus_version().to_string(),
        };

        let prover = lumen_zkml::mock::for_types::<RoutingWitness, RoutingPublicInputs>();
        let proof = prover.prove(&self.proving_vk, &public, &witness)?;
        let verification = verify_with_witness(&self.proving_vk, &public, &witness, &proof)?;

        Ok(StepResult {
            completion,
            tool_output,
            routing: RoutingDecision { public, witness },
            proof,
            verification,
            defense_verdict,
        })
    }
}

impl AgentRuntimeBuilder {
    /// defense 엔진 제공 (기본값은 [`DefenseEngine::default`]).
    pub fn defense(mut self, engine: DefenseEngine) -> Self {
        self.defense = Some(engine);
        self
    }

    /// 추론 엔진 제공.
    pub fn inference(mut self, engine: Arc<dyn InferenceEngine>) -> Self {
        self.inference = Some(engine);
        self
    }

    /// 도구 삽입. `with_tools` 의 편의 함수.
    pub fn tool(mut self, tool: crate::tool::Tool) -> Result<Self> {
        self.tools.register(tool)?;
        Ok(self)
    }

    /// 등록부 통째 교체.
    pub fn tools(mut self, registry: ToolRegistry) -> Self {
        self.tools = registry;
        self
    }

    /// 주어진 도구에 capability 바인드. 같은 도구 반복 시 덮어씁니다.
    pub fn capability(mut self, tool: ToolId, cap: Capability) -> Self {
        self.caps_by_tool.insert(tool, cap);
        self
    }

    /// 증명 검증 키 제공 (mock 백엔드).
    pub fn proving_vk(mut self, vk: MockVk) -> Self {
        self.proving_vk = Some(vk);
        self
    }

    /// 정책 스냅샷 해시 제공.
    pub fn policy_hash(mut self, hash: Blake3Hash) -> Self {
        self.policy_hash = Some(hash);
        self
    }

    /// sampling 파라미터 재정의.
    pub fn sampling(mut self, params: SamplingParams) -> Self {
        self.sampling = params;
        self
    }

    /// 생성 마무리. 필수 필드가 빠지면 에러.
    pub fn build(self) -> Result<AgentRuntime> {
        let inference = self
            .inference
            .ok_or_else(|| Error::Invalid("AgentRuntime::inference required".into()))?;
        let proving_vk = self
            .proving_vk
            .ok_or_else(|| Error::Invalid("AgentRuntime::proving_vk required".into()))?;
        let policy_hash = self
            .policy_hash
            .ok_or_else(|| Error::Invalid("AgentRuntime::policy_hash required".into()))?;
        Ok(AgentRuntime {
            agent: self.agent,
            policy: self.policy,
            caps_by_tool: self.caps_by_tool,
            defense: self.defense.unwrap_or_default(),
            inference,
            tools: self.tools,
            proving_vk,
            policy_hash,
            sampling: self.sampling,
        })
    }
}
