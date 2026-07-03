//! Mock commitment prover.
//!
//! **이는 ZK 증명이 아닙니다.** 무거운 백엔드를 사용할 수 없는 환경에서도
//! [`crate::ProvingSystem`] trait 를 끝-끝으로 - 서명, 전송, 검증자 wiring
//! 까지 - 연습할 수 있도록 하는 투명한 콘텐츠-바인딩 commitment 입니다.
//!
//! "증명" 은 `BLAKE3(circuit_id || public_postcard || witness_postcard)`.
//! 검증자는 동일한 입력으로 그 해시를 재계산하고 불일치 시 거부합니다.
//! 검증자가 witness 를 필요로 하므로 본 프로토콜은 **영지식 속성을 전혀
//! 제공하지 않으며**, 약한 soundness 만 - 유일한 보장은 `(public, witness)`
//! 양쪽을 모두 가진 누군가가 증명을 생성했다는 사실 - 만 제공합니다.

use elib_blake::Blake3;
use lumen_core::{Blake3Hash, Error, Result};
use serde::{Deserialize, Serialize};

use crate::{ProvingSystem, Verification};

/// Mock 백엔드의 검증 키 - 회로 식별자만 보유.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MockVk {
    /// 회로 식별자 (예: `"lumen.routing.v1"`).
    pub circuit_id: String,
}

/// Mock 증명 산출물.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MockProof {
    /// 회로, 공개 입력, witness 를 바인드한 commit 다이제스트.
    pub digest: Blake3Hash,
}

/// Mock prover - 무상태.
#[derive(Clone, Debug, Default)]
pub struct MockCommitmentProver;

impl MockCommitmentProver {
    /// 새 인스턴스.
    pub fn new() -> Self {
        Self
    }

    fn bind<W, P>(circuit_id: &str, public: &P, witness: &W) -> Result<Blake3Hash>
    where
        W: Serialize,
        P: Serialize,
    {
        let public_bytes =
            postcard::to_allocvec(public).map_err(|e| Error::Decode(format!("public: {e}")))?;
        let witness_bytes =
            postcard::to_allocvec(witness).map_err(|e| Error::Decode(format!("witness: {e}")))?;
        let mut hasher = Blake3::new();
        hasher.update(b"lumen.mock.commit.v1\x00");
        hasher.update(&u64::to_le_bytes(circuit_id.len() as u64));
        hasher.update(circuit_id.as_bytes());
        hasher.update(&u64::to_le_bytes(public_bytes.len() as u64));
        hasher.update(&public_bytes);
        hasher.update(&u64::to_le_bytes(witness_bytes.len() as u64));
        hasher.update(&witness_bytes);
        let buf = hasher
            .finalize()
            .map_err(|e| Error::Zkml(format!("blake3 finalize: {e:?}")))?;
        let mut out = [0u8; 32];
        out.copy_from_slice(buf.as_slice());
        Ok(Blake3Hash(out))
    }
}

/// `(Witness, PublicInputs)` 가 trait 차원의 타입 파라미터입니다.
impl<W, P> ProvingSystem for MockCommitmentProverFor<W, P>
where
    W: Serialize + Send + Sync,
    P: Serialize + Send + Sync,
{
    type Witness = W;
    type PublicInputs = P;
    type Proof = MockProof;
    type Vk = MockVk;

    fn setup(&self, circuit_id: &str) -> Result<Self::Vk> {
        Ok(MockVk {
            circuit_id: circuit_id.to_string(),
        })
    }

    fn prove(
        &self,
        vk: &Self::Vk,
        public: &Self::PublicInputs,
        witness: &Self::Witness,
    ) -> Result<Self::Proof> {
        let digest = MockCommitmentProver::bind(&vk.circuit_id, public, witness)?;
        Ok(MockProof { digest })
    }

    fn verify(
        &self,
        vk: &Self::Vk,
        public: &Self::PublicInputs,
        proof: &Self::Proof,
    ) -> Result<Verification> {
        // mock 검증자는 재계산을 위해 witness 가 필요합니다 - 그러나 진짜
        // 검증자는 그것을 받아들이면 안 되므로 여기서는 거부합니다. 대신
        // 공개 입력 자체를 sentinel "witness" 로 넣어 다시 binding 한 뒤
        // prover 가 같은 형태를 사용했는지를 요구합니다.
        //
        // 정직한 mock 파이프라인은 `prove` + `verify_with_witness` 입니다;
        // 노출된 `verify` API 는 다이제스트가 `(circuit_id, public_inputs)`
        // 에 대해 *well-formed* 인지 - 즉 공개 입력에 대해 비-malleable 인지
        // - 정도만 확인할 수 있습니다. 이 차이를 분명히 하기 위해 PI-only
        // 다이제스트를 계산한 뒤 일치 여부에 따라 항상
        // `CommitmentOnly`/`Invalid` 를 반환합니다.
        // 실제 검증에는 여전히 witness 가 필요합니다.
        let pi_only = MockCommitmentProver::bind(&vk.circuit_id, public, &EmptyWitness)?;
        let _ = pi_only;
        // witness 가 없으면 원본 다이제스트를 재계산할 수 없으므로 여기서는
        // `Invalid` 를 반환합니다. 에이전트 런타임은 witness 가 손에 있을 때
        // `verify_with_witness` (아래 확장 trait) 를 호출해야 합니다. 진짜
        // 백엔드에서는 이런 우회가 필요 없습니다.
        let _ = proof;
        Err(Error::Zkml(
            "mock backend requires `verify_with_witness`; the witnessless `verify` is intentionally not supported"
                .into(),
        ))
    }
}

/// (Witness, PublicInputs) 타입을 파라미터로 운반하는 무상태 타입.
///
/// [`MockCommitmentProver`] 자체가 type-erased 이므로 generic 으로 만들려면
/// 모든 호출 사이트가 아직 모르는 타입을 핀해야 합니다. 이 wrapper 를 거치면
/// trait impl 을 얻습니다.
#[derive(Clone, Debug, Default)]
pub struct MockCommitmentProverFor<W, P>(std::marker::PhantomData<fn(W, P)>);

impl<W, P> MockCommitmentProverFor<W, P> {
    /// 생성.
    pub fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

/// 특정 witness/public-input 쌍에 특화된 정규 mock prover 를 반환하는 헬퍼 alias.
pub fn for_types<W, P>() -> MockCommitmentProverFor<W, P> {
    MockCommitmentProverFor::new()
}

/// witness 가 없는 곳에서 prover 를 얻기 위한 marker witness.
#[derive(Serialize, Deserialize)]
struct EmptyWitness;

/// witness 를 명시적으로 제공해 mock 증명을 검증합니다.
///
/// 진짜 백엔드에는 필요 없는 우회 - mock 의 경우 binding 을 재계산하는
/// 유일한 방법입니다. 일치 시 [`Verification::CommitmentOnly`] 를 반환하며
/// **절대** [`Verification::ZkVerified`] 를 반환하지 않습니다.
pub fn verify_with_witness<W, P>(
    vk: &MockVk,
    public: &P,
    witness: &W,
    proof: &MockProof,
) -> Result<Verification>
where
    W: Serialize,
    P: Serialize,
{
    let expected = MockCommitmentProver::bind(&vk.circuit_id, public, witness)?;
    if expected == proof.digest {
        Ok(Verification::CommitmentOnly)
    } else {
        Ok(Verification::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Clone)]
    struct W {
        a: u32,
        b: u32,
    }
    #[derive(Serialize, Deserialize, Clone)]
    struct P {
        public: u32,
    }

    #[test]
    fn happy_path_returns_commitment_only() {
        let prover: MockCommitmentProverFor<W, P> = for_types();
        let vk = prover.setup("lumen.routing.v1").unwrap();
        let public = P { public: 7 };
        let witness = W { a: 1, b: 2 };
        let proof = prover.prove(&vk, &public, &witness).unwrap();
        let v = verify_with_witness(&vk, &public, &witness, &proof).unwrap();
        assert_eq!(v, Verification::CommitmentOnly);
        assert_ne!(v, Verification::ZkVerified, "must never claim ZK");
    }

    #[test]
    fn tampered_proof_is_invalid() {
        let prover: MockCommitmentProverFor<W, P> = for_types();
        let vk = prover.setup("lumen.routing.v1").unwrap();
        let public = P { public: 7 };
        let witness = W { a: 1, b: 2 };
        let mut proof = prover.prove(&vk, &public, &witness).unwrap();
        proof.digest.0[0] ^= 0xFF;
        let v = verify_with_witness(&vk, &public, &witness, &proof).unwrap();
        assert_eq!(v, Verification::Invalid);
    }

    #[test]
    fn different_witness_same_public_invalid() {
        let prover: MockCommitmentProverFor<W, P> = for_types();
        let vk = prover.setup("lumen.routing.v1").unwrap();
        let public = P { public: 7 };
        let w1 = W { a: 1, b: 2 };
        let w2 = W { a: 9, b: 9 };
        let proof = prover.prove(&vk, &public, &w1).unwrap();
        let v = verify_with_witness(&vk, &public, &w2, &proof).unwrap();
        assert_eq!(v, Verification::Invalid);
    }

    #[test]
    fn different_circuit_id_invalid() {
        let prover: MockCommitmentProverFor<W, P> = for_types();
        let vk_a = prover.setup("a").unwrap();
        let vk_b = prover.setup("b").unwrap();
        let public = P { public: 1 };
        let witness = W { a: 1, b: 1 };
        let proof = prover.prove(&vk_a, &public, &witness).unwrap();
        let v = verify_with_witness(&vk_b, &public, &witness, &proof).unwrap();
        assert_eq!(v, Verification::Invalid);
    }

    #[test]
    fn determinism() {
        let prover: MockCommitmentProverFor<W, P> = for_types();
        let vk = prover.setup("lumen.routing.v1").unwrap();
        let public = P { public: 7 };
        let witness = W { a: 1, b: 2 };
        let p1 = prover.prove(&vk, &public, &witness).unwrap();
        let p2 = prover.prove(&vk, &public, &witness).unwrap();
        assert_eq!(p1, p2, "mock prover must be deterministic");
    }
}
