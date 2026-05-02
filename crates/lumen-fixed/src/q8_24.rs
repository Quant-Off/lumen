//! Q8.24 고정소수점: signed 32 비트, 24 비트 분수부.
//!
//! 범위 ≈ ±128.0, 해상도 ≈ 6e-8. 작은 범위 안에서 높은 정밀도가 필요한
//! 정규화된 weight (예: 재스케일 후의 softmax 출력) 에 적합합니다.

use core::cmp::Ordering;
use core::fmt;
use core::ops::{Add, Mul, Neg, Sub};

/// Q8.24 signed 고정소수점.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Q8_24(pub i32);

impl Q8_24 {
    /// 분수부 비트 수.
    pub const FRAC_BITS: u32 = 24;
    /// 이 표현에서의 `1.0`.
    pub const ONE: Self = Self(1 << Self::FRAC_BITS);
    /// `0.0`.
    pub const ZERO: Self = Self(0);
    /// 표현 가능한 최솟값.
    pub const MIN: Self = Self(i32::MIN);
    /// 표현 가능한 최댓값.
    pub const MAX: Self = Self(i32::MAX);

    /// 작은 signed 정수로부터 생성 (범위 ±127); 그 외에는 saturate.
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

    /// 0 방향 잘린 정수 부분.
    pub const fn to_i32(self) -> i32 {
        self.0 >> Self::FRAC_BITS
    }

    /// 원시 표현.
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Saturating 덧셈.
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating 뺄셈.
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// 64 비트 중간을 사용한 saturating 곱셈.
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

    /// Checked 덧셈.
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked 곱셈.
    pub const fn checked_mul(self, other: Self) -> Option<Self> {
        let prod = (self.0 as i64) * (other.0 as i64);
        let shifted = prod >> Self::FRAC_BITS;
        if shifted > i32::MAX as i64 || shifted < i32::MIN as i64 {
            None
        } else {
            Some(Self(shifted as i32))
        }
    }
}

impl Add for Q8_24 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.saturating_add(rhs)
    }
}

impl Sub for Q8_24 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.saturating_sub(rhs)
    }
}

impl Mul for Q8_24 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.saturating_mul(rhs)
    }
}

impl Neg for Q8_24 {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.saturating_neg())
    }
}

impl PartialOrd for Q8_24 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Q8_24 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Debug for Q8_24 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Q8_24(raw={})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_round_trip() {
        assert_eq!(Q8_24::ONE.to_i32(), 1);
    }

    #[test]
    fn mul_one_is_identity() {
        let v = Q8_24::from_i32(3);
        assert_eq!(v * Q8_24::ONE, v);
    }

    #[test]
    fn checked_overflow() {
        assert!(Q8_24::MAX.checked_add(Q8_24::ONE).is_none());
    }
}
