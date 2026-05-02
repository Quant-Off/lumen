//! Lumen 의 증명-바인딩 런타임 경로를 위한 결정론적 고정소수점 산술.
//!
//! 하드웨어 부동소수점은 CPU 간 비결정적 (denormals, FMA contraction, x87
//! 의 80-bit 중간 표현) 이며, 이 비결정성은 ZK 증명의 재현성을 깨뜨립니다.
//! 이 크레이트는 런타임 시 절대 FPU 를 건드리지 않는 Q 형식 정수 타입을
//! 노출합니다.
//!
//! 가이드라인:
//!
//! - activation 과 도구 라우팅 점수에는 [`Q16_16`] 사용 (범위 ±2^15).
//! - 정규화된 weight 에는 [`Q8_24`] 사용 (범위 ±2^7, 더 정밀).
//! - `f32` 변환은 `calibration` feature 뒤로만 - ZK 증명을 발행하는 바이너리
//!   에서는 절대 활성화하지 마세요.
//!
//! Saturating 연산이 기본 `add`/`sub`/`mul` 입니다. 명시적으로 overflow 를
//! 감지해야 한다면 `checked_*` 패밀리를 사용하세요.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod q16_16;
pub mod q8_24;
pub mod quant;

pub use q16_16::Q16_16;
pub use q8_24::Q8_24;
pub use quant::{quantize_i8, QuantParams};
