//! 온체인 검증기 (EVM / Mina) scaffold 생성과 배포 자동화.
//!
//! v0.3 의 `Halo2Prover` 는 binary-argmax routing 회로의 *제약 만족* 을 평문
//! witness 로 검증합니다 (MockProver). 이 크레이트는 동일한 제약을 두 가지
//! 온체인 환경에서 재현하는 **검증 contract** 를 emit 합니다:
//!
//! - **EVM**: Solidity `RoutingVerifier.sol` + Foundry 배포 스크립트.
//! - **Mina**: o1js `RoutingVerifier.ts` + 배포 entry.
//!
//! 둘 다 `score_a, score_b, choice_bit, selected_value` 의 두 가지 게이트
//! (`selected = bit*(b-a) + a` 와 `bit ∈ {0,1}`) 를 강제합니다. 이는 v0.3
//! 의 halo2 backend 와 *bit-동일* 한 의미를 갖도록 의도적으로 동일한
//! 회로 식별자 (`circuit_id`) 와 함께 emit 됩니다 - v0.4 후속에서 KZG 기반
//! succinct proof 로 업그레이드되어도 검증기 ABI 가 깨지지 않도록 회로
//! 식별자가 contract storage 에 박혀 발행됩니다.
//!
//! ## 배포 자동화
//!
//! [`emit::emit_artifacts`] 가 모든 source 와 deploy script 를 디스크에
//! 작성합니다. 사용자가 `lumen verifier deploy --chain evm --rpc <url>` 로
//! 실행하면 [`deploy::deploy_evm`] 이 `forge create` (또는 `cast send`) 를
//! 자식 프로세스로 호출합니다. 외부 도구가 없으면 *DRY-RUN* 모드로 명령만
//! 출력합니다 - 에어갭 환경에서 배포 명령을 텍스트로 복사해 운반하는
//! 워크플로우와도 호환됩니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod deploy;
pub mod emit;
pub mod evm;
pub mod mina;

use serde::{Deserialize, Serialize};

/// 검증기를 emit 할 체인 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Chain {
    /// EVM (Solidity).
    Evm,
    /// Mina (o1js / SnarkyJS).
    Mina,
}

impl std::str::FromStr for Chain {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "evm" | "ethereum" | "eth" => Ok(Self::Evm),
            "mina" | "o1js" => Ok(Self::Mina),
            other => Err(format!("알 수 없는 chain: {other}")),
        }
    }
}

/// 검증기 emit 시 contract 메타에 박힐 회로 식별자.
///
/// 이 값이 contract storage / 이벤트 로그에 새겨져, 후속 KZG-backed prover
/// 로 업그레이드되어도 동일한 식별자를 사용하면 ABI 가 안정적입니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierMeta {
    /// halo2 backend 와 일치하는 회로 식별자 (예: `lumen.routing.binary.v1`).
    pub circuit_id: String,
    /// emit 시각 (free-form ISO-ish 문자열; 결정성 빌드에서는 호출자가
    /// 0 으로 고정).
    pub emitted_at: String,
}

impl VerifierMeta {
    /// 회로 식별자만 받는 편의 생성자.
    pub fn for_circuit(circuit_id: impl Into<String>) -> Self {
        Self {
            circuit_id: circuit_id.into(),
            emitted_at: "deterministic".to_string(),
        }
    }
}
