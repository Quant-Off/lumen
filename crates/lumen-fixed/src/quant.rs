//! 정수 양자화 헬퍼. 모든 연산은 결정론적이며 부동소수점 산술이 없습니다.

use alloc::vec::Vec;

use crate::Q16_16;

/// Symmetric int8 양자화 파라미터.
///
/// 실수 도메인 매핑은 `q = clamp(round(x / scale) + zero_point, -128, 127)`
/// 이며, 모두 `Q16_16` 위에서 정수 산술로 수행됩니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuantParams {
    /// Q16.16 형식의 scale 인자. 양수여야 합니다.
    pub scale: Q16_16,
    /// Zero point 오프셋. 호출자가 임의의 int8 또는 0 을 전달할 수 있도록
    /// `i32` 가 아닌 `i8` 로 받습니다.
    pub zero_point: i8,
}

/// `Q16_16` 슬라이스를 주어진 파라미터로 `i8` 로 양자화합니다.
///
/// ±127 에서 saturating clamp. 새로 할당된 vector 를 반환하며 입력은
/// 절대 변경되지 않습니다.
pub fn quantize_i8(values: &[Q16_16], params: &QuantParams) -> Vec<i8> {
    let mut out = Vec::with_capacity(values.len());
    let inv_scale_raw = params.scale.raw();
    debug_assert!(inv_scale_raw > 0, "scale must be positive");
    for v in values {
        // q = round(v / scale) + zp
        // 보정 시점에 보통 scale 의 역수를 미리 계산해 두지만 여기서는
        // 호출자가 `scale` 자체를 직접 전달하므로 나눗셈을 수행합니다.
        let v_raw = i64::from(v.raw());
        let s_raw = i64::from(inv_scale_raw);
        // (a + s/2) 의 signed division 으로 round-half-to-even 흉내.
        let half = s_raw / 2;
        let numerator = if v_raw >= 0 {
            v_raw + half
        } else {
            v_raw - half
        };
        let q = numerator / s_raw;
        let q = q.saturating_add(i64::from(params.zero_point));
        let clamped = q.clamp(i8::MIN as i64, i8::MAX as i64) as i8;
        out.push(clamped);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn quantize_identity_scale_one() {
        let scale = Q16_16::ONE; // 1.0
        let params = QuantParams {
            scale,
            zero_point: 0,
        };
        let xs = [
            Q16_16::from_i32(0),
            Q16_16::from_i32(1),
            Q16_16::from_i32(-1),
            Q16_16::from_i32(127),
            Q16_16::from_i32(-128),
        ];
        let q = quantize_i8(&xs, &params);
        assert_eq!(q, vec![0, 1, -1, 127, -128]);
    }

    #[test]
    fn quantize_clamps_at_127() {
        let scale = Q16_16::ONE;
        let params = QuantParams {
            scale,
            zero_point: 0,
        };
        let q = quantize_i8(&[Q16_16::from_i32(500)], &params);
        assert_eq!(q, vec![127]);
    }

    #[test]
    fn quantize_with_zero_point() {
        let scale = Q16_16::ONE;
        let params = QuantParams {
            scale,
            zero_point: 10,
        };
        let q = quantize_i8(&[Q16_16::from_i32(0), Q16_16::from_i32(5)], &params);
        assert_eq!(q, vec![10, 15]);
    }
}
