//! Q16.16 고정소수점: signed 32 비트, 16 비트 분수부.
//!
//! 범위 ≈ ±32_768.0, 해상도 ≈ 1.5e-5.

use core::cmp::Ordering;
use core::fmt;
use core::ops::{Add, Mul, Neg, Sub};

/// Q16.16 signed 고정소수점.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Q16_16(pub i32);

impl Q16_16 {
    /// 분수부 비트 수.
    pub const FRAC_BITS: u32 = 16;
    /// 고정소수점 표현의 `1.0`.
    pub const ONE: Self = Self(1 << Self::FRAC_BITS);
    /// `0.0`.
    pub const ZERO: Self = Self(0);
    /// 표현 가능한 최솟값.
    pub const MIN: Self = Self(i32::MIN);
    /// 표현 가능한 최댓값.
    pub const MAX: Self = Self(i32::MAX);

    /// signed 정수로부터 생성 (타입 경계에서 saturate 가능).
    pub const fn from_i32(v: i32) -> Self {
        let shifted = (v as i64) << Self::FRAC_BITS;
        if shifted > i32::MAX as i64 {
            Self::MAX
        } else if shifted < i32::MIN as i64 {
            Self::MIN
        } else {
            Self(shifted as i32)
        }
    }

    /// 0 방향으로 잘린 정수 부분.
    pub const fn to_i32(self) -> i32 {
        self.0 >> Self::FRAC_BITS
    }

    /// 원시 표현으로부터 생성 (시프트 없음).
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// 내부 정수 표현.
    pub const fn raw(self) -> i32 {
        self.0
    }

    // -------- saturating 산술 (기본 `+`/`-`/`*`) --------

    /// Saturating 덧셈.
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating 뺄셈.
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Saturating 곱셈.
    ///
    /// 64 비트 중간 곱셈을 수행한 뒤 다시 Q 표현으로 시프트하며 overflow
    /// 시 saturate 합니다.
    pub const fn saturating_mul(self, other: Self) -> Self {
        let prod = (self.0 as i64) * (other.0 as i64);
        let shifted = prod >> Self::FRAC_BITS;
        if shifted > i32::MAX as i64 {
            Self::MAX
        } else if shifted < i32::MIN as i64 {
            Self::MIN
        } else {
            Self(shifted as i32)
        }
    }

    // -------- checked 산술 --------

    /// Checked 덧셈; overflow 시 `None`.
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked 뺄셈; overflow 시 `None`.
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked 곱셈; Q 시프트 후 overflow 시 `None`.
    pub const fn checked_mul(self, other: Self) -> Option<Self> {
        let prod = (self.0 as i64) * (other.0 as i64);
        let shifted = prod >> Self::FRAC_BITS;
        if shifted > i32::MAX as i64 || shifted < i32::MIN as i64 {
            None
        } else {
            Some(Self(shifted as i32))
        }
    }

    // -------- f32 브릿지 (calibration 전용) --------

    /// `f32` 로부터 변환.
    ///
    /// **증명 경로에서는 절대 호출하지 마세요.** 부동소수점 변환은 호스트
    /// 마다 비결정적입니다. 감사를 쉽게 하기 위해 feature gate 가 걸려
    /// 있습니다.
    #[cfg(feature = "calibration")]
    pub fn from_f32(v: f32) -> Self {
        let scaled = v * (1u32 << Self::FRAC_BITS) as f32;
        let clamped = scaled.clamp(i32::MIN as f32, i32::MAX as f32);
        Self(clamped as i32)
    }

    /// `f32` 로 변환 (calibration 전용 - [`Self::from_f32`] 참고).
    #[cfg(feature = "calibration")]
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / (1u32 << Self::FRAC_BITS) as f32
    }
}

impl From<i32> for Q16_16 {
    fn from(v: i32) -> Self {
        Self::from_i32(v)
    }
}

impl Add for Q16_16 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.saturating_add(rhs)
    }
}

impl Sub for Q16_16 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.saturating_sub(rhs)
    }
}

impl Mul for Q16_16 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.saturating_mul(rhs)
    }
}

impl Neg for Q16_16 {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.saturating_neg())
    }
}

impl PartialOrd for Q16_16 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Q16_16 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Debug for Q16_16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let int_part = self.0 >> Self::FRAC_BITS;
        let frac_part = (self.0 as u32) & ((1u32 << Self::FRAC_BITS) - 1);
        write!(
            f,
            "Q16_16({}.{:05})",
            int_part,
            frac_part * 100_000 / (1 << Self::FRAC_BITS)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_round_trip() {
        assert_eq!(Q16_16::ONE.to_i32(), 1);
        assert_eq!(Q16_16::from_i32(7).to_i32(), 7);
    }

    #[test]
    fn saturating_add_at_max() {
        let max = Q16_16::MAX;
        assert_eq!((max + Q16_16::ONE), Q16_16::MAX);
    }

    #[test]
    fn saturating_sub_at_min() {
        let min = Q16_16::MIN;
        assert_eq!((min - Q16_16::ONE), Q16_16::MIN);
    }

    #[test]
    fn mul_precision_two_times_one_half() {
        // 2 * 0.5 = 1.0
        let two = Q16_16::from_i32(2);
        let half = Q16_16(1 << (Q16_16::FRAC_BITS - 1));
        assert_eq!(two * half, Q16_16::ONE);
    }

    #[test]
    fn checked_overflow() {
        assert!(Q16_16::MAX.checked_add(Q16_16::ONE).is_none());
        assert!(Q16_16::MIN.checked_sub(Q16_16::ONE).is_none());
        assert!(Q16_16::MAX.checked_mul(Q16_16::from_i32(2)).is_none());
    }

    #[test]
    fn ordering() {
        assert!(Q16_16::from_i32(1) < Q16_16::from_i32(2));
        assert!(Q16_16::from_i32(-1) < Q16_16::ZERO);
    }
}
