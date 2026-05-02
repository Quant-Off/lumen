//! wasmtime 위에 구축된 Lumen 에이전트용 WASM 샌드박스.
//!
//! ## 결정성
//!
//! [`SandboxConfig::deterministic_engine`] 은 다음 wasmtime `Engine` 을
//! 반환합니다.
//!
//! - SIMD, threads, relaxed-SIMD **비활성화**,
//! - Cranelift NaN canonicalisation **활성화**,
//! - fuel + epoch interruption 으로 메모리와 실행 시간을 제한.
//!
//! `lumen-fixed` 가 강제하는 정수 전용 관행과 결합하면, 이 샌드박스 안에서
//! 실행되는 에이전트 WASM 모듈은 CPU 벤더에 무관하게 byte-동일한 결과를
//! 생성합니다.
//!
//! ## 권한 분리
//!
//! 호스트 임포트는 *capability-gated* 입니다 - 각 호출 사이트는
//! [`crate::host::HostState`] 의 capability 를 조회한 뒤 부재 시 임포트를
//! 거부합니다. 묵시적 권한은 없습니다 - 의미 있는 모든 액션은 정책 엔진이
//! 서명한 명시적인 capability 를 필요로 합니다.
//!
//! ## v0 범위
//!
//! - [`Sandbox::run_module`] 모듈을 로드하고 `_start` 를 실행하고 반환.
//! - 호스트 임포트: `lumen_log`, `lumen_call_tool` (cap-gated), `lumen_recv`.
//! - 진짜 Rust->WASM 에이전트 컴파일 파이프라인은 v0.3 의 `lumen-sdk` 와
//!   `agents/echo-agent/` 에서 완성됨; 여기에는 `lumen_log` 를 끝-끝으로
//!   exercise 하는 `.wat` 테스트 fixture 도 함께 동봉되어 있습니다.

#![warn(missing_docs)]

pub mod config;
pub mod host;
pub mod imports;
pub mod runner;

pub use config::SandboxConfig;
pub use host::{HostState, ToolCallRecord};
pub use runner::Sandbox;
