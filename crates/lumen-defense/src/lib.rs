//! 프롬프트 인젝션 / 자일브레이크 방어 엔진.
//!
//! 세 단계 순차 파이프라인이며, 모두 첫 사용 시점에 [`once_cell`] 뒤에서
//! 사전 컴파일됩니다.
//!
//! 1. **Lexicon (Aho-Corasick)** - 약 30개의 알려진 자일브레이크 트리거
//!    ("ignore previous instructions", "DAN mode", role-override 시퀀스, …)
//!    의 내장 코퍼스. 상용 하드웨어에서 sub-µs/kB 수준.
//! 2. **Regex** - 단순 리터럴로 표현 불가능한 고차 패턴 (예:
//!    `(?i)system\s*:.*`) 을 위한 [`regex::RegexSet`].
//! 3. **Heuristic** - non-printable byte 비율, base64-likeness, 의심스러운
//!    길이. 즉각 차단보다 soft `Suspect{score}` 를 반환.
//!
//! 설계자는 audit 로그에 corpus 버전 핀 ([`DefenseEngine::corpus_version`])
//! 을 포함시켜 다운스트림 검증자가 verdict 를 재현할 수 있게 해야 합니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod engine;
pub mod heuristics;
pub mod lexicon;
pub mod regex_stage;

pub use engine::{BlockReason, DefenseEngine, Verdict};
