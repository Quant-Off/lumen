//! 도구 라우팅을 위해 증명 시스템(proving system)에 제공되는 witness/public-inputs 쌍.
//!
//! 공개 입력(Public inputs):
//! - `circuit_id` (검증 키에 포함되어 있으며, 여기서는 중복 정의하지 않음)
//! - `prompt_hash` - (방어 엔진에 의해 정제된) 프롬프트의 Blake3 해시
//! - `policy_hash` - 정책 스냅샷의 Blake3 해시
//! - `tool_id` - 선택된 도구 ID (64바이트 이하의 문자열)
//!
//! 증거(Witness):
//! - `args_hash` - 인자(args) JSON의 Blake3 해시
//! - `defense_corpus` - 재현성을 위해 사용된 어휘집(lexicon)의 지문(fingerprint)

use lumen_core::{Blake3Hash, ToolId};
use serde::{Deserialize, Serialize};

/// 검증자가 확인할 수 있는 공개 입력입니다. 리플레이 시에도 변하지 않으며,
/// 검증자가 합의해야 하는 모든 항목을 결합합니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingPublicInputs {
    /// 에이전트가 추론한 프롬프트의 해시입니다.
    pub prompt_hash: Blake3Hash,
    /// 정책 엔진에서 사용된 정책 스냅샷의 해시입니다.
    pub policy_hash: Blake3Hash,
    /// 에이전트가 선택한 도구입니다. `None`은 "도구 없음 - 텍스트 전용 응답"을 의미합니다.
    pub tool: Option<ToolId>,
}

/// 증거(Witness) 데이터 - 실제 영지식(ZK) 환경에서는 제3자 검증자에게 절대 공개되지 않습니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingWitness {
    /// 도구에 전달된 JSON 인자의 해시입니다. 텍스트 전용 응답의 경우 `None`입니다.
    pub args_hash: Option<Blake3Hash>,
    /// 라우팅 시점의 방어 엔진 코퍼스(corpus) 지문입니다.
    pub defense_corpus: String,
}

/// [`crate::StepResult`]와 함께 반환되는 통합 기록입니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// 공개 컴포넌트입니다.
    pub public: RoutingPublicInputs,
    /// 증거(Witness) 컴포넌트입니다.
    pub witness: RoutingWitness,
}
