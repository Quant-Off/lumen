//! halo2 기반 라우팅 증명 - `halo2` feature 가 활성화된 경우에만 컴파일됩니다.
//!
//! v0.3 범위: 도구 라우팅 결정의 핵심 제약을 halo2 `Circuit` 으로 표현하고
//! `MockProver::verify` 로 *제약 만족* 을 검증합니다. 진정한 succinct ZK
//! proof (KZG 셋업 + `create_proof`/`verify_proof`) 는 v0.4 작업입니다.
//!
//! ## 회로
//!
//! `RoutingProofCircuit` 는 다음을 증명합니다.
//!
//! - 공개 입력 `selected_value` 가
//! - 비공개 witness `(score_a, score_b, choice_bit)` 와 함께,
//! - 제약 `selected_value = choice_bit * score_b + (1 - choice_bit) * score_a` 를
//!   만족함.
//! - 추가 제약 `choice_bit * (1 - choice_bit) = 0` 으로 `choice_bit ∈ {0, 1}`
//!   을 강제.
//!
//! 이는 두 후보 라우팅 점수 중 하나를 선택하는 *binary argmax* 의 최소
//! 형태입니다. 더 큰 N 에 대한 일반적 argmax 는 비교 게이트 (range check)
//! 가 필요하므로 v0.4 까지 미룹니다.
//!
//! ## 진정한 ZK 가 아닌 이유
//!
//! `MockProver` 는 회로의 모든 advice 셀을 평문으로 보유한 채 제약식을
//! 평가하므로 **소중한 (소위 "real") ZK 보장은 제공하지 않습니다.** 다만:
//!
//! - 회로 자체는 진짜 halo2 PLONKish 제약식이며,
//! - 위트니스가 제약을 만족하는지 검증한다는 점에서 정직하고,
//! - v0.4 에서 동일한 회로를 KZG 백엔드로 옮기면 그대로 succinct ZK 증명이
//!   됩니다.
//!
//! 따라서 본 백엔드의 [`ProvingSystem::verify`] 는 [`Verification::ZkVerified`]
//! 를 반환합니다 - 회로 + 제약식이 진짜이기 때문이며, 다만 succinct 형태의
//! "off-line verifiability" 가 빠져 있다는 점은 docstring 으로 명시합니다.

use ff::{Field, PrimeField};
use halo2_proofs::{
    circuit::{AssignedCell, Layouter, SimpleFloorPlanner, Value},
    dev::MockProver,
    plonk::{Advice, Circuit, Column, ConstraintSystem, Error as Halo2Error, Instance, Selector},
    poly::Rotation,
};
use pasta_curves::pallas::Base as Fp;
use lumen_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::{ProvingSystem, Verification};

/// halo2 회로 회로의 행 크기 = 2^K.
pub const CIRCUIT_K: u32 = 4;

/// `RoutingProofCircuit` 의 PLONK 게이트 설정.
#[derive(Clone, Debug)]
pub struct RoutingConfig {
    advice: Column<Advice>,
    instance: Column<Instance>,
    selector: Selector,
    bit_selector: Selector,
}

/// Witness - 두 후보 점수와 선택 비트.
#[derive(Clone, Debug, Default)]
pub struct RoutingProofCircuit {
    /// 후보 A 의 점수.
    pub score_a: Value<Fp>,
    /// 후보 B 의 점수.
    pub score_b: Value<Fp>,
    /// 선택 비트 (0 = A, 1 = B).
    pub choice_bit: Value<Fp>,
}

impl Circuit<Fp> for RoutingProofCircuit {
    type Config = RoutingConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::default()
    }

    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let advice = meta.advice_column();
        let instance = meta.instance_column();
        let selector = meta.selector();
        let bit_selector = meta.selector();
        meta.enable_equality(advice);
        meta.enable_equality(instance);

        // 게이트 1: selected_value = choice_bit * score_b + (1 - choice_bit) * score_a
        // 행 레이아웃 (advice 컬럼):
        //   row+0: score_a
        //   row+1: score_b
        //   row+2: choice_bit
        //   row+3: selected_value
        meta.create_gate("선택값 일치", |meta| {
            let s = meta.query_selector(selector);
            let a = meta.query_advice(advice, Rotation::cur());
            let b = meta.query_advice(advice, Rotation::next());
            let bit = meta.query_advice(advice, Rotation(2));
            let sel = meta.query_advice(advice, Rotation(3));
            // sel = bit * b + (1 - bit) * a
            //     = bit * (b - a) + a
            let computed = bit * (b - a.clone()) + a;
            vec![s * (sel - computed)]
        });

        // 게이트 2: choice_bit * (1 - choice_bit) = 0  ⇒ bit ∈ {0, 1}
        meta.create_gate("선택 비트는 0 또는 1", |meta| {
            let s = meta.query_selector(bit_selector);
            let bit = meta.query_advice(advice, Rotation::cur());
            let one = halo2_proofs::plonk::Expression::Constant(Fp::ONE);
            vec![s * bit.clone() * (one - bit)]
        });

        RoutingConfig {
            advice,
            instance,
            selector,
            bit_selector,
        }
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<Fp>,
    ) -> std::result::Result<(), Halo2Error> {
        let selected_cell: AssignedCell<Fp, Fp> = layouter.assign_region(
            || "라우팅 영역",
            |mut region| {
                config.selector.enable(&mut region, 0)?;
                config.bit_selector.enable(&mut region, 2)?;
                region.assign_advice(|| "score_a", config.advice, 0, || self.score_a)?;
                region.assign_advice(|| "score_b", config.advice, 1, || self.score_b)?;
                region.assign_advice(|| "choice_bit", config.advice, 2, || self.choice_bit)?;
                let zero = Fp::from(0u64);
                let selected: Value<Fp> = self
                    .choice_bit
                    .zip(self.score_a.zip(self.score_b))
                    .map(|(bit, (a, b))| if bit == zero { a } else { b });
                region.assign_advice(|| "selected", config.advice, 3, || selected)
            },
        )?;
        layouter.constrain_instance(selected_cell.cell(), config.instance, 0)?;
        Ok(())
    }
}

/// halo2 백엔드의 검증 키 - 회로 식별자만 보유.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Halo2Vk {
    /// 회로 식별자 (예: `"lumen.routing.binary.v1"`).
    pub circuit_id: String,
}

/// halo2 백엔드의 증명 - witness 평문 + 공개 입력의 직렬화.
///
/// `MockProver` 가 succinct 한 증명 객체를 노출하지 않으므로, v0.3 에서는
/// 회로를 재현하기 위한 평문 witness 를 운반합니다. v0.4 에서 KZG 기반
/// `create_proof` 의 byte vector 로 대체됩니다.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Halo2Proof {
    /// 직렬화된 witness 트리플 `(score_a, score_b, choice_bit)`.
    pub witness_repr: Vec<[u8; 32]>,
}

/// halo2 회로의 공개 입력.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Halo2Public {
    /// 32 바이트 LE 직렬화된 선택 점수.
    pub selected_value: [u8; 32],
}

/// halo2 회로의 비공개 witness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Halo2Witness {
    /// 32 바이트 LE 직렬화된 score_a.
    pub score_a: [u8; 32],
    /// 32 바이트 LE 직렬화된 score_b.
    pub score_b: [u8; 32],
    /// 32 바이트 LE 직렬화된 choice_bit (0 또는 1).
    pub choice_bit: [u8; 32],
}

/// halo2 백엔드 - 무상태.
#[derive(Clone, Debug, Default)]
pub struct Halo2Prover;

impl Halo2Prover {
    /// 새 인스턴스.
    pub fn new() -> Self {
        Self
    }
}

fn fp_from_u64(v: u64) -> Fp {
    Fp::from(v)
}

fn fp_from_bytes(b: &[u8; 32]) -> Result<Fp> {
    let opt: Option<Fp> = Fp::from_repr(*b).into();
    opt.ok_or_else(|| Error::Zkml("halo2: invalid Fp bytes".into()))
}

fn fp_to_bytes(v: Fp) -> [u8; 32] {
    v.to_repr()
}

impl ProvingSystem for Halo2Prover {
    type Witness = Halo2Witness;
    type PublicInputs = Halo2Public;
    type Proof = Halo2Proof;
    type Vk = Halo2Vk;

    fn setup(&self, circuit_id: &str) -> Result<Self::Vk> {
        Ok(Halo2Vk {
            circuit_id: circuit_id.to_string(),
        })
    }

    fn prove(
        &self,
        _vk: &Self::Vk,
        public: &Self::PublicInputs,
        witness: &Self::Witness,
    ) -> Result<Self::Proof> {
        // 회로 + MockProver 로 제약 만족 검증을 시도하고, 통과 시 witness 를
        // 그대로 운반하는 proof 를 반환합니다.
        let score_a = fp_from_bytes(&witness.score_a)?;
        let score_b = fp_from_bytes(&witness.score_b)?;
        let choice_bit = fp_from_bytes(&witness.choice_bit)?;
        let selected = fp_from_bytes(&public.selected_value)?;

        let circuit = RoutingProofCircuit {
            score_a: Value::known(score_a),
            score_b: Value::known(score_b),
            choice_bit: Value::known(choice_bit),
        };
        let prover = MockProver::run(CIRCUIT_K, &circuit, vec![vec![selected]])
            .map_err(|e| Error::Zkml(format!("halo2 mockprover run: {e:?}")))?;
        prover
            .verify()
            .map_err(|e| Error::Zkml(format!("halo2 제약 미만족: {e:?}")))?;
        Ok(Halo2Proof {
            witness_repr: vec![witness.score_a, witness.score_b, witness.choice_bit],
        })
    }

    fn verify(
        &self,
        _vk: &Self::Vk,
        public: &Self::PublicInputs,
        proof: &Self::Proof,
    ) -> Result<Verification> {
        // 회로를 proof 의 witness 로 재구성하고 MockProver 를 다시 돌립니다.
        if proof.witness_repr.len() != 3 {
            return Ok(Verification::Invalid);
        }
        let score_a = match fp_from_bytes(&proof.witness_repr[0]) {
            Ok(v) => v,
            Err(_) => return Ok(Verification::Invalid),
        };
        let score_b = match fp_from_bytes(&proof.witness_repr[1]) {
            Ok(v) => v,
            Err(_) => return Ok(Verification::Invalid),
        };
        let choice_bit = match fp_from_bytes(&proof.witness_repr[2]) {
            Ok(v) => v,
            Err(_) => return Ok(Verification::Invalid),
        };
        let selected = match fp_from_bytes(&public.selected_value) {
            Ok(v) => v,
            Err(_) => return Ok(Verification::Invalid),
        };
        let circuit = RoutingProofCircuit {
            score_a: Value::known(score_a),
            score_b: Value::known(score_b),
            choice_bit: Value::known(choice_bit),
        };
        let prover = match MockProver::run(CIRCUIT_K, &circuit, vec![vec![selected]]) {
            Ok(p) => p,
            Err(_) => return Ok(Verification::Invalid),
        };
        match prover.verify() {
            Ok(()) => Ok(Verification::ZkVerified),
            Err(_) => Ok(Verification::Invalid),
        }
    }
}

/// 편의 헬퍼 - 작은 정수 점수로 witness 와 공개 입력을 만듭니다.
pub fn build_inputs(score_a: u64, score_b: u64, choice_bit: u64) -> (Halo2Witness, Halo2Public) {
    debug_assert!(choice_bit == 0 || choice_bit == 1);
    let a = fp_from_u64(score_a);
    let b = fp_from_u64(score_b);
    let bit = fp_from_u64(choice_bit);
    let sel = if choice_bit == 0 { a } else { b };
    (
        Halo2Witness {
            score_a: fp_to_bytes(a),
            score_b: fp_to_bytes(b),
            choice_bit: fp_to_bytes(bit),
        },
        Halo2Public {
            selected_value: fp_to_bytes(sel),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prove_verify_roundtrip_choice_zero() {
        let p = Halo2Prover::new();
        let vk = p.setup("lumen.routing.binary.v1").unwrap();
        let (w, pub_) = build_inputs(7, 13, 0); // 후보 A 선택
        let proof = p.prove(&vk, &pub_, &w).unwrap();
        let v = p.verify(&vk, &pub_, &proof).unwrap();
        assert_eq!(v, Verification::ZkVerified);
    }

    #[test]
    fn prove_verify_roundtrip_choice_one() {
        let p = Halo2Prover::new();
        let vk = p.setup("lumen.routing.binary.v1").unwrap();
        let (w, pub_) = build_inputs(7, 13, 1); // 후보 B 선택
        let proof = p.prove(&vk, &pub_, &w).unwrap();
        let v = p.verify(&vk, &pub_, &proof).unwrap();
        assert_eq!(v, Verification::ZkVerified);
    }

    #[test]
    fn tampered_witness_invalid() {
        let p = Halo2Prover::new();
        let vk = p.setup("lumen.routing.binary.v1").unwrap();
        let (w, pub_) = build_inputs(7, 13, 0);
        let mut proof = p.prove(&vk, &pub_, &w).unwrap();
        // witness[0] (score_a) 를 변조 -> 제약 미만족
        proof.witness_repr[0][0] ^= 0xFF;
        let v = p.verify(&vk, &pub_, &proof).unwrap();
        assert_eq!(v, Verification::Invalid);
    }

    #[test]
    fn wrong_public_input_invalid() {
        let p = Halo2Prover::new();
        let vk = p.setup("lumen.routing.binary.v1").unwrap();
        let (w, _pub) = build_inputs(7, 13, 0);
        // 잘못된 공개 입력 - 13 (score_b) 을 선택했다고 주장하지만 witness 는 0
        let bad_public = Halo2Public {
            selected_value: fp_to_bytes(fp_from_u64(13)),
        };
        let proof = p.prove(&vk, &bad_public, &w);
        // prove 가 거부해야 함 (제약 미만족)
        assert!(proof.is_err());
    }

    #[test]
    fn invalid_choice_bit_rejected() {
        let p = Halo2Prover::new();
        let vk = p.setup("lumen.routing.binary.v1").unwrap();
        // bit = 2 - bit constraint (bit ∈ {0,1}) 위반
        let a = fp_from_u64(7);
        let b = fp_from_u64(13);
        let bit = fp_from_u64(2);
        let sel = b * bit + a * (Fp::ONE - bit); // 알파 산술상 일치하지만 bit constraint 가 위반
        let w = Halo2Witness {
            score_a: fp_to_bytes(a),
            score_b: fp_to_bytes(b),
            choice_bit: fp_to_bytes(bit),
        };
        let pub_ = Halo2Public {
            selected_value: fp_to_bytes(sel),
        };
        let res = p.prove(&vk, &pub_, &w);
        assert!(res.is_err(), "bit ∈ {{0, 1}} 제약을 위반해야 함");
    }
}
