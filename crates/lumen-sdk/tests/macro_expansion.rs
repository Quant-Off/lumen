//! `#[lumen_agent]` proc-macro 가 정상적으로 expand 되는지 컴파일-only
//! 검증.
//!
//! 실제 `_start` 는 호스트 타겟에서 `lumen_sdk::log` 에 도달하면 panic 하므로
//! 호출하지 않습니다. 이 테스트의 목적은 매크로 출력이 컴파일되는지 (이름,
//! 옵션 파싱, 시그니처 검증) 만 확인하는 것입니다.
//!
//! 두 개의 `_start` 가 같은 link 단위에 함께 들어가면 중복 심볼이 되므로,
//! 가능한 시그니처 변형들을 cfg gate 로 분리해 *컴파일 검증* 만 받습니다.

#![cfg(feature = "macros")]

use lumen_sdk::lumen_agent;

#[lumen_agent(name = "echo-agent", version = "0.4.0")]
fn step() {
    // 사용자 단계.
}

#[test]
fn user_function_remains_callable() {
    // proc-macro 가 사용자 함수를 그대로 보존했는지 호출로 확인.
    step();
}
