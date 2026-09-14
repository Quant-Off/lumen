//! 플러거블 증명 시스템 추상화.
//!
//! Lumen 은 *선택적* 영지식 증명을 목표로 합니다: 전체 LLM forward pass 를
//! 증명하는 대신, 에이전트의 **도구 라우팅** 결정과 **출력 필터링 / 정책
//! 준수** 로직을 증명합니다. 이들은 훨씬 작은 회로이므로 halo2 급 증명도
//! 실행 가능합니다.
//!
//! [`ProvingSystem`] trait 가 백엔드 선택을 에이전트 런타임으로부터 숨깁니다.
//! 두 가지 백엔드가 scaffold 되어 있습니다:
//!
//! - **`mock`** - 항상 사용 가능. `(circuit_id, public_inputs, witness)` 를
//!   바인드하는 BLAKE3 commitment 를 생성합니다. 검증은 binding 을 다시
//!   계산해 일치 시 [`Verification::CommitmentOnly`] 를 반환. **ZK 증명이
//!   아닙니다.** Verdict variant 는 의도적으로 [`Verification::ZkVerified`]
//!   와 분리되어 있어 호출 사이트가 두 가지를 절대 혼동할 수 없습니다.
//! - **`ezkl`** - feature-gated stub.
//!
//! ## 폐쇄형(Air-Gapped) 환경 메모
//!
//! v0.3 까지 존재하던 `halo2` feature 와 그 PLONK 라우팅 회로는 v0.4 에서
//! 제거되었습니다. `halo2_proofs` / `pasta_curves` 등 타원곡선 의존성이
//! 매우 무거우면서도 `MockProver` 검증 단계에서는 witness 가 평문으로
//! 노출되어 ZK 보장 자체가 없었기 때문입니다. 향후 succinct ZK 가 필요해
//! 지면 SP1 / RISC Zero 등 별도 프레임워크 채택 여부를 마일스톤 재검토
//! 시점에 결정합니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod mock;
pub mod verification;

#[cfg(feature = "ezkl")]
pub mod ezkl;

use lumen_core::Result;
use serde::{de::DeserializeOwned, Serialize};

pub use mock::{MockCommitmentProver, MockProof, MockVk};
pub use verification::Verification;

/// 백엔드-비의존 증명 시스템.
///
/// 구현체가 `Witness`, `PublicInputs`, `Proof`, `Vk` 의 구체 의미를 결정합니다.
/// 에이전트 런타임은 핫패스에서 boxed trait object 로 운반하므로
/// (`Box<dyn ProvingSystem<...>>`) 백엔드 교체가 소스 레이아웃 비용 없이
/// 가능합니다.
pub trait ProvingSystem: Send + Sync {
    /// 회로에 입력되는 비공개 witness.
    type Witness: Serialize + Send + Sync;
    /// 검증자에게 보이는 공개 입력.
    type PublicInputs: Serialize + Send + Sync;
    /// 인코딩된 증명 산출물.
    type Proof: Serialize + DeserializeOwned + Clone + Send + Sync;
    /// 검증 키.
    type Vk: Clone + Send + Sync;

    /// 주어진 회로 식별자에 대한 검증 키를 계산합니다.
    ///
    /// 진짜 백엔드에서는 structured reference string 과 회로별 셋업이
    /// 수반되지만, mock 에서는 한 줄짜리입니다.
    fn setup(&self, circuit_id: &str) -> Result<Self::Vk>;

    /// `public` 과 `witness` 를 묶는 증명을 생성합니다.
    fn prove(
        &self,
        vk: &Self::Vk,
        public: &Self::PublicInputs,
        witness: &Self::Witness,
    ) -> Result<Self::Proof>;

    /// 공개 입력과 증명을 검증합니다.
    fn verify(
        &self,
        vk: &Self::Vk,
        public: &Self::PublicInputs,
        proof: &Self::Proof,
    ) -> Result<Verification>;
}
