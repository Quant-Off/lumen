//! Lumen 에이전트 런타임입니다.
//!
//! [`AgentRuntime::step`]는 다음의 정규 파이프라인을 실행합니다.
//!
//! 1. **Defense** - [`lumen_defense::DefenseEngine::analyze`].
//! 2. **Inference** - [`lumen_inference::InferenceEngine::complete`].
//! 3. **Policy** - [`lumen_capability::PolicyEngine::check`]에 tool 호출.
//! 4. **Tool exec** - [`tool::ToolHandler`] 등록.
//! 5. **ZK bind** - [`route::RoutingDecision`]을 기록하고 이를 [`lumen_zkml::ProvingSystem`]에 입력하여 증명을 생성하고 검증합니다.
//!
//! 출력값은 추론 결과(completion), 선택적인 도구 실행 결과, 그리고 증명 및 검증 결과가 포함된 [`StepResult`]입니다.
//! 동일한 입력(프롬프트, 정책, 역량, RNG 시드)이 주어지면 전체 단계는 결정론적(deterministic)으로 동작합니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod route;
pub mod runtime;
pub mod tool;

pub use route::{RoutingDecision, RoutingPublicInputs, RoutingWitness};
pub use runtime::{AgentRuntime, StepResult};
pub use tool::{Tool, ToolHandler, ToolRegistry};
