//! ezkl 기반 증명 시스템. **v0.3 에서 미구현.**

use lumen_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::{ProvingSystem, Verification};

/// stub ezkl prover.
#[derive(Clone, Debug, Default)]
pub struct EzklProver;

/// stub 검증 키.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EzklVk;

/// stub 증명.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EzklProof;

impl<W, P> ProvingSystem for EzklStub<W, P>
where
    W: serde::Serialize + Send + Sync,
    P: serde::Serialize + Send + Sync,
{
    type Witness = W;
    type PublicInputs = P;
    type Proof = EzklProof;
    type Vk = EzklVk;

    fn setup(&self, _circuit_id: &str) -> Result<Self::Vk> {
        Err(Error::NotImplemented("ezkl::setup"))
    }
    fn prove(
        &self,
        _vk: &Self::Vk,
        _public: &Self::PublicInputs,
        _witness: &Self::Witness,
    ) -> Result<Self::Proof> {
        Err(Error::NotImplemented("ezkl::prove"))
    }
    fn verify(
        &self,
        _vk: &Self::Vk,
        _public: &Self::PublicInputs,
        _proof: &Self::Proof,
    ) -> Result<Verification> {
        Err(Error::NotImplemented("ezkl::verify"))
    }
}

/// 타입 파라미터화된 wrapper, mock prover 와 동일한 레이아웃.
#[derive(Clone, Debug, Default)]
pub struct EzklStub<W, P>(std::marker::PhantomData<fn(W, P)>);
