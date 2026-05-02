//! 가벼운 통계 휴리스틱. 어느 것도 단독으로 *차단* 하지 않으며, [`crate::Verdict::Suspect`] 로 기울어지는 누적 의심 점수에만 기여합니다.

/// 비-인쇄, 비-공백 바이트 비율 점수 (0..=100).
pub fn non_printable_score(input: &str) -> u8 {
    if input.is_empty() {
        return 0;
    }
    let bad = input
        .bytes()
        .filter(|&b| !(b.is_ascii_graphic() || b == b' ' || b == b'\n' || b == b'\t' || b == b'\r'))
        .count();
    let pct = bad * 100 / input.len();
    (pct.min(100)) as u8
}

/// 긴 base64 블롭과의 유사성 점수 (0..=100).
///
/// 디코딩은 *하지 않으며*, 단지 base64 같은 문자가 길게 이어지는 구간을
/// 표시하여 프롬프트로 삽입된 바이너리 페이로드를 알립니다.
pub fn base64_like_score(input: &str) -> u8 {
    let mut max_run: usize = 0;
    let mut run: usize = 0;
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 0;
        }
    }
    if max_run < 64 {
        return 0;
    }
    let pct = (max_run.saturating_sub(64)) * 100 / 256;
    pct.min(100) as u8
}

/// 과도한 길이 점수 (0..=100).
pub fn length_score(input: &str, soft_cap: usize) -> u8 {
    if input.len() <= soft_cap {
        return 0;
    }
    let over = input.len() - soft_cap;
    (over.min(soft_cap) * 100 / soft_cap.max(1)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_printable_low_for_text() {
        assert_eq!(non_printable_score("hello world"), 0);
    }

    #[test]
    fn non_printable_high_for_garbage() {
        let mut s = String::new();
        for b in 0u8..32 {
            s.push(b as char);
        }
        assert!(non_printable_score(&s) > 50);
    }

    #[test]
    fn base64_score_zero_for_short() {
        assert_eq!(base64_like_score("abcd"), 0);
    }
}
