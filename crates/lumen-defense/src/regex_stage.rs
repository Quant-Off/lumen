//! 방어 엔진의 regex 단계.

use once_cell::sync::Lazy;
use regex::RegexSet;

/// 리터럴 lexicon 으로 표현하기 어려운 regex 패턴들. 대소문자 무시
/// (관련 패턴은 `(?i)` 를 직접 포함).
pub const REGEX_PATTERNS: &[&str] = &[
    r"(?i)\bbase64[\s_-]*decode\b.*\bexec\b",
    r"(?i)\bdecode\b.*\bexecute\b",
    r"\\x[0-9a-fA-F]{2}.*\\x[0-9a-fA-F]{2}.*\\x[0-9a-fA-F]{2}",
    r"(?i)\beval\s*\(.*\)",
    r"(?i)<\s*script[^>]*>",
    r"(?i)\bossystem\s*\(",
    r"(?i)\bos\s*\.\s*system\b",
    r"(?i)curl\s+-?[a-z]*\s+https?://",
    r"(?i)wget\s+https?://",
    r"(?i)\bsudo\b\s+rm\s+-rf",
];

static SET: Lazy<RegexSet> =
    Lazy::new(|| RegexSet::new(REGEX_PATTERNS).expect("regex set must compile"));

/// 매칭된 첫 regex 인덱스 (있다면). audit 로그에 적합합니다.
pub fn first_match(input: &str) -> Option<usize> {
    let m = SET.matches(input);
    m.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_eval() {
        assert!(first_match("then eval(do_evil_things())").is_some());
    }

    #[test]
    fn passes_benign() {
        assert!(first_match("evaluate the trade-off").is_none());
    }
}
