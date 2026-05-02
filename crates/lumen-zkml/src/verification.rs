//! 검증 verdict.

use serde::{Deserialize, Serialize};

/// 증명 검증 결과.
///
/// 두 variant 는 의도적으로 **호환되지 않습니다.** `CommitmentOnly` 는 절대
/// `ZkVerified` 로 대체될 수 없습니다 - 그러면 진짜 ZK 보장을 "증명자와
/// 검증자가 같은 바이트에 합의한다" 정도로 조용히 격하시키게 됩니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verification {
    /// 백엔드가 실제 영지식 증명을 만들었고 검증자가 받아들였습니다.
    ZkVerified,
    /// 증명이 비-ZK commitment (예: mock prover) 이며 binding 이 재현됨.
    /// **호출자는 이를 무결성 전용으로만 취급해야 하며**, soundness 나
    /// 영지식 보장으로 취급하면 안 됩니다.
    CommitmentOnly,
    /// 검증 실패 (변조된 증명, 잘못된 입력 등).
    Invalid,
}

impl Verification {
    /// verdict 가 (어떤 종류든) 받아들여진 증명을 의미하면 `true`.
    pub fn is_accepted(self) -> bool {
        matches!(self, Self::ZkVerified | Self::CommitmentOnly)
    }
}
