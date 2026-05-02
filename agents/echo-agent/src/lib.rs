//! Lumen 데모 에이전트 - `wasm32-unknown-unknown` 으로 컴파일됩니다.
//!
//! 호스트가 `_start` 를 호출하면 에이전트는 다음 순서로 동작합니다.
//!
//! 1. `lumen.log("hello-from-rust-wasm")` 로 audit 한 줄 기록.
//! 2. `lumen.call_tool("echo", {"text": "hi"})` 호출 (Capability 검증 통과 시).
//! 3. `call_tool` 결과에 따라 audit 추가 기록.
//!
//! Capability 가 없으면 `Denied` 가 떨어지며 에이전트가 자체 audit 으로
//! 그 사실을 기록합니다 (호스트 측에서도 거부 audit 이 별도로 남습니다).

#![no_std]
#![no_main]
// 패닉 핸들러 외 다른 모든 unsafe 차단.
#![deny(unsafe_op_in_unsafe_fn)]

use core::panic::PanicInfo;

use lumen_sdk::{call_tool, log, LogLevel, ToolError};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    log(LogLevel::Error, "agent panic");
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() {
    log(LogLevel::Info, "echo-agent starting");
    let result = call_tool("echo", "{\"text\":\"hi from wasm-agent\"}");
    match result {
        Ok(()) => log(LogLevel::Info, "echo tool call ok"),
        Err(ToolError::Denied) => log(LogLevel::Warn, "echo tool denied (no capability)"),
        Err(ToolError::Memory) => log(LogLevel::Error, "echo tool memory error"),
        Err(ToolError::Unknown(_)) => log(LogLevel::Error, "echo tool unknown rc"),
    }
    log(LogLevel::Info, "echo-agent done");
}
