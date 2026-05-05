//! `AgentRuntime` - defense -> infer -> policy -> tool -> prove 를 오케스트레이션합니다.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::stream;
use lumen_capability::{Action, Capability, PolicyEngine};
use lumen_core::{AgentId, Blake3Hash, Error, Result, Timestamp, ToolId};
use lumen_defense::{DefenseEngine, Verdict};
use lumen_inference::{
    Completion, FinishReason, InferenceEngine, SamplingParams, StreamingEngine, Token, TokenStream,
};
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

/// 스트리밍 step 에서 발생하는 이벤트.
///
/// [`AgentRuntime::stream_step`] 이 반환하는 스트림은 이 열거형을 아이템으로
/// 방출합니다. 호출자는 토큰을 실시간으로 처리하고, 스트림이 끝날 때
/// [`StreamEvent::Complete`] 에서 최종 결과를 얻습니다.
#[derive(Clone, Debug)]
pub enum StreamEvent {
    /// 생성된 단일 토큰.
    Token(Token),
    /// 스트리밍 완료 — defense, 도구 실행, ZK 증명을 포함한 최종 결과.
    Complete(StepResult),
}

/// 런타임이 생성 시점에 필요로 하는 입력들.
pub struct AgentRuntime {
    agent: AgentId,
    policy: Arc<PolicyEngine>,
    /// 도구 id 키의 capability; 런타임은 호출당 하나를 사용합니다.
    caps_by_tool: BTreeMap<ToolId, Capability>,
    defense: DefenseEngine,
    inference: Arc<dyn InferenceEngine>,
    /// 스트리밍을 지원하는 경우 설정됩니다.
    streaming: Option<Arc<dyn StreamingEngine>>,
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
    streaming: Option<Arc<dyn StreamingEngine>>,
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
            streaming: None,
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
        self.finalize_step(completion, defense_verdict, prompt).await
    }

    /// 스트리밍 step 실행.
    ///
    /// [`StreamingEngine`] 이 설정되지 않은 경우 [`Error::NotImplemented`].
    ///
    /// 반환된 스트림은 [`StreamEvent::Token`] 을 방출하고, 마지막에
    /// [`StreamEvent::Complete`] 를 방출합니다. 스트림을 drop 하면 생성이
    /// 취소됩니다 (단, ZK 증명은 발생하지 않습니다).
    pub async fn stream_step(
        &self,
        prompt: &str,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent>> + '_> {
        let streaming_engine = self.streaming.as_ref().ok_or_else(|| {
            Error::NotImplemented(
                "AgentRuntime: 스트리밍 엔진이 설정되지 않았습니다".into(),
            )
        })?;

        // 1. Defense.
        let defense_verdict = self.defense.analyze(prompt);
        if let Verdict::Block(reason) = &defense_verdict {
            tracing::warn!(target: "lumen.agent", ?reason, "stream_step blocked");
            return Err(Error::Defense(format!("{reason:?}")));
        }

        // 2. 스트리밍 시작
        let token_stream: TokenStream = streaming_engine
            .stream_complete(prompt, &self.sampling)
            .await?;

        let prompt = prompt.to_owned();
        let defense_verdict_clone = defense_verdict.clone();

        // 3. 토큰 스트림을 tee: 호출자에게 Token 이벤트 방출 + 텍스트 누적
        let event_stream = build_stream(
            token_stream,
            prompt,
            defense_verdict_clone,
            self,
        );

        Ok(event_stream)
    }

    /// 완성된 텍스트에 대해 정책, 도구 실행, ZK 증명을 수행합니다.
    async fn finalize_step(
        &self,
        completion: Completion,
        defense_verdict: Verdict,
        prompt: &str,
    ) -> Result<StepResult> {
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

    /// 엔진이 스트리밍을 지원하는지 여부.
    pub fn supports_streaming(&self) -> bool {
        self.streaming.is_some()
    }
}

/// 토큰 스트림에서 `StreamEvent` 스트림을 생성합니다.
///
/// 이 함수는 생성 루프와 ZK 파이프라인을 연결합니다:
/// 1. 각 토큰을 `StreamEvent::Token` 으로 방출합니다.
/// 2. 스트림 종료 후 tool / policy / ZK 를 실행하고 `StreamEvent::Complete` 를 방출합니다.
fn build_stream<'a>(
    token_stream: TokenStream,
    prompt: String,
    defense_verdict: Verdict,
    runtime: &'a AgentRuntime,
) -> impl futures::Stream<Item = Result<StreamEvent>> + 'a {
    // 단계별로 폴링하는 상태 기계 스트림.
    // State 0: 토큰 방출 중 (텍스트 누적)
    // State 1: 완료 이벤트 방출
    // State 2: 종료
    enum State {
        Streaming {
            token_stream: TokenStream,
            accumulated: String,
        },
        Finalizing {
            accumulated: String,
        },
        Done,
    }

    let initial = State::Streaming {
        token_stream,
        accumulated: String::new(),
    };

    stream::unfold(
        (initial, prompt, defense_verdict),
        move |(state, prompt, verdict)| {
            let runtime_ref = runtime;
            async move {
                match state {
                    State::Streaming {
                        mut token_stream,
                        mut accumulated,
                    } => {
                        use futures::StreamExt as _;
                        match token_stream.next().await {
                            Some(Ok(token)) => {
                                accumulated.push_str(&token.text);
                                let is_last = token.finish_reason.is_some();
                                let event = StreamEvent::Token(token);
                                let next_state = if is_last {
                                    State::Finalizing { accumulated }
                                } else {
                                    State::Streaming {
                                        token_stream,
                                        accumulated,
                                    }
                                };
                                Some((Ok(event), (next_state, prompt, verdict)))
                            }
                            Some(Err(e)) => {
                                Some((Err(e), (State::Done, prompt, verdict)))
                            }
                            None => {
                                // 스트림이 FinishReason 없이 종료된 경우도 처리.
                                let state = State::Finalizing { accumulated };
                                Some((
                                    // 빈 dummy 이벤트 대신 바로 finalize 로 전환합니다.
                                    // 다음 poll 에서 Complete 를 방출합니다.
                                    Ok(StreamEvent::Token(Token {
                                        id: 0,
                                        text: String::new(),
                                        logprob: None,
                                        finish_reason: Some(FinishReason::Eos),
                                    })),
                                    (state, prompt, verdict),
                                ))
                            }
                        }
                    }
                    State::Finalizing { accumulated } => {
                        let completion = Completion {
                            text: accumulated,
                            tool_call: None,
                        };
                        let result = runtime_ref
                            .finalize_step(completion, verdict.clone(), &prompt)
                            .await;
                        match result {
                            Ok(step) => Some((
                                Ok(StreamEvent::Complete(step)),
                                (State::Done, prompt, verdict),
                            )),
                            Err(e) => {
                                Some((Err(e), (State::Done, prompt, verdict)))
                            }
                        }
                    }
                    State::Done => None,
                }
            }
        },
    )
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

    /// 스트리밍 추론 엔진 제공.
    ///
    /// 스트리밍 엔진을 제공하면 [`AgentRuntime::stream_step`] 이 활성화됩니다.
    pub fn streaming_engine(mut self, engine: Arc<dyn StreamingEngine>) -> Self {
        self.streaming = Some(engine);
        self
    }

    /// 도구 삽입.
    pub fn tool(mut self, tool: crate::tool::Tool) -> Result<Self> {
        self.tools.register(tool)?;
        Ok(self)
    }

    /// 등록부 통째 교체.
    pub fn tools(mut self, registry: ToolRegistry) -> Self {
        self.tools = registry;
        self
    }

    /// 주어진 도구에 capability 바인드.
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
            streaming: self.streaming,
            tools: self.tools,
            proving_vk,
            policy_hash,
            sampling: self.sampling,
        })
    }
}
