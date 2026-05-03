//! `#[lumen_agent]` proc-macro 시연 에이전트.
//!
//! `_start` ABI 정의를 매크로가 자동 생성하므로 사용자 코드는 도메인
//! 로직만 작성합니다. 호스트 audit 로그에는 다음 라인이 남습니다:
//!
//! ```text
//! agent=macro-agent version=0.4.0
//! lumen_agent: done
//! ```

#![no_std]
#![no_main]
#![deny(unsafe_op_in_unsafe_fn)]

use core::panic::PanicInfo;

use lumen_sdk::{call_tool, log, lumen_agent, LogLevel, ToolError};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    log(LogLevel::Error, "agent panic");
    loop {}
}

#[lumen_agent(name = "macro-agent", version = "0.4.0")]
fn step() {
    match call_tool("echo", "{\"text\":\"hi from macro-agent\"}") {
        Ok(()) => log(LogLevel::Info, "echo ok"),
        Err(ToolError::Denied) => log(LogLevel::Warn, "echo denied"),
        Err(_) => log(LogLevel::Error, "echo other"),
    }
}
