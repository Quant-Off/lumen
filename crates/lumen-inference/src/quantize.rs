//! 추론 백엔드용 양자화 설정.
//!
//! Lumen 은 두 가지 양자화 경로를 제공합니다:
//!
//! - **GGUF** (`QuantizationKind::Gguf`): candle-transformers / llama.cpp 의
//!   GGUF 포맷. 추론 속도를 극대화하며, ZK 경로 밖에서 실행됩니다.
//! - **FixedPoint** (`QuantizationKind::FixedPoint`): `lumen-fixed` 의 Q-형식
//!   정수 산술. 연산이 ZK witness 에 바인딩되어야 하는 도구 라우팅 경로에
//!   사용됩니다. ZK 바이너리에서는 FPU 를 절대 건드리지 않습니다.
//! - **Int8**: candle 의 i8 텐서. 빠르지만 결정론 보장 없음.

use serde::{Deserialize, Serialize};

/// GGUF 가중치 양자화 레벨.
///
/// 레벨이 낮을수록 메모리 사용량이 감소하고 정확도도 소폭 감소합니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GgufLevel {
    /// 4-bit (Q4_0) — 7B 모델 기준 약 3.9 GB.
    Q4K0,
    /// 4-bit (Q4_1) — Q4K0 대비 정확도 소폭 향상.
    Q4K1,
    /// 5-bit (Q5_0).
    Q5K0,
    /// 5-bit (Q5_1).
    Q5K1,
    /// 8-bit (Q8_0) — 7B 모델 기준 약 7 GB. fp16 의 약 60% 메모리.
    Q8K0,
    /// fp16 가중치 — 양자화 없음, 최대 정확도.
    F16,
}

/// `lumen-fixed` 고정소수점 정밀도.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixedPointPrecision {
    /// Q16.16 — activation 및 도구 라우팅 점수용 (범위 ±2^15).
    Q16_16,
    /// Q8.24 — 정규화된 weight 용 (범위 ±2^7, 더 정밀).
    Q8_24,
}

/// 추론 백엔드에 전달되는 양자화 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantizationKind {
    /// 양자화 없음 — f32 full precision.
    None,
    /// candle i8 텐서 (빠르지만 비결정론적).
    Int8,
    /// ZK 증명 경로용 `lumen-fixed` 고정소수점.
    FixedPoint(FixedPointPrecision),
    /// GGUF 포맷 가중치 (candle-transformers / llama.cpp).
    Gguf(GgufLevel),
}

/// 추론 백엔드에 전달되는 양자화 설정.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuantizationConfig {
    /// 사용할 양자화 종류.
    pub kind: QuantizationKind,
}

impl Default for QuantizationConfig {
    fn default() -> Self {
        Self {
            kind: QuantizationKind::None,
        }
    }
}

impl QuantizationConfig {
    /// GGUF Q4_K0 — 일반적으로 7B 모델의 균형잡힌 기본 선택.
    pub fn gguf_q4() -> Self {
        Self {
            kind: QuantizationKind::Gguf(GgufLevel::Q4K0),
        }
    }

    /// GGUF Q8_K0 — 높은 정확도가 필요할 때.
    pub fn gguf_q8() -> Self {
        Self {
            kind: QuantizationKind::Gguf(GgufLevel::Q8K0),
        }
    }

    /// ZK 증명 경로용 Q16.16 고정소수점.
    pub fn zk_fixed() -> Self {
        Self {
            kind: QuantizationKind::FixedPoint(FixedPointPrecision::Q16_16),
        }
    }
}
