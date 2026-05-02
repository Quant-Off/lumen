//! Lumen WASM 에이전트 SDK.
//!
//! 이 크레이트는 `wasm32-unknown-unknown` 타겟으로 컴파일되어 Lumen 샌드박스
//! 안에서 실행될 에이전트가 사용하는 얇은 래퍼입니다. 두 가지 호스트 임포트를
//! 노출합니다.
//!
//! - [`log`] - `lumen_log(level, ptr, len)` 래퍼. Capability 가 필요 없으며
//!   호스트 audit 에 한 줄 기록합니다.
//! - [`call_tool`] - `lumen_call_tool(tool_ptr, tool_len, args_ptr, args_len) -> i32`
//!   래퍼. 호스트의 `PolicyEngine` 가 호출 시점의 Capability 를 검증한 후에만
//!   실제 도구를 실행합니다.
//!
//! `alloc` 의존성이 없으므로 글로벌 할당자가 없는 가장 미니멀한 wasm 에이전트도
//! 빌드할 수 있습니다. 동적 문자열 조립이 필요한 경우 `alloc` feature 를
//! 켜고 [`announce`] 같은 보조 함수를 사용하세요.
//!
//! 비-`wasm32` 타겟에서도 컴파일은 되지만 (그래야 `cargo check` 등이 다른
//! 타겟에서 동작합니다) 실제 호스트 호출 부분은 `panic!` 합니다 - SDK 의 호출
//! 의도는 WASM 환경 안에서만 의미를 가지기 때문입니다.

#![no_std]
#![warn(missing_docs)]

#[cfg(feature = "alloc")]
extern crate alloc;

/// 호스트 audit 로그의 레벨.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    /// 디버깅용 상세 메시지.
    Trace = 0,
    /// 정상 동작 흐름.
    Info = 1,
    /// 정책상 의심스럽지만 차단되지는 않은 사건.
    Warn = 2,
    /// 정책 위반이나 오류.
    Error = 3,
}

/// `lumen_call_tool` 호출 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolError {
    /// 호스트 `PolicyEngine` 가 Capability 검증을 거부했습니다.
    Denied,
    /// 호스트 측 메모리 변환 실패 (포인터/길이 범위 초과 등).
    Memory,
    /// 호스트가 알려지지 않은 코드를 반환했습니다.
    Unknown(i32),
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "lumen")]
unsafe extern "C" {
    fn lumen_log(level: i32, ptr: i32, len: i32);
    fn lumen_call_tool(tool_ptr: i32, tool_len: i32, args_ptr: i32, args_len: i32) -> i32;
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn lumen_log(_level: i32, _ptr: i32, _len: i32) {
    panic!("lumen-sdk: 호스트 임포트는 wasm32-unknown-unknown 외부에서 호출할 수 없습니다");
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn lumen_call_tool(
    _tool_ptr: i32,
    _tool_len: i32,
    _args_ptr: i32,
    _args_len: i32,
) -> i32 {
    panic!("lumen-sdk: 호스트 임포트는 wasm32-unknown-unknown 외부에서 호출할 수 없습니다");
}

/// 호스트 audit 로그에 한 줄 기록합니다.
///
/// Capability 가 필요 없습니다. 호스트의 `lumen_log` 가 메시지 바이트를 즉시
/// 호스트 메모리로 복사하므로 호출 후 `msg` 의 수명에 의존하지 않습니다.
pub fn log(level: LogLevel, msg: &str) {
    let bytes = msg.as_bytes();
    // SAFETY: bytes.as_ptr() points to `bytes.len()` valid bytes inside the
    // wasm linear memory. The host copies them out before returning.
    unsafe {
        lumen_log(level as i32, bytes.as_ptr() as i32, bytes.len() as i32);
    }
}

/// 호스트에 등록된 도구를 호출합니다.
///
/// `tool` 은 호스트 `ToolRegistry` 의 키, `args_json` 은 도구가 기대하는
/// JSON 인자입니다. 호스트는 (1) 인자 바이트를 호스트 메모리로 복사하고,
/// (2) 호출자의 Capability 를 `PolicyEngine::check` 로 검증한 뒤,
/// (3) 통과 시에만 `ToolHandler::call` 을 호출합니다.
///
/// 반환값은 호스트가 도구 실행 결과를 받았는지 여부만을 나타냅니다. 결과
/// 본문은 향후 `lumen_recv` 호스트 임포트로 회수합니다 (v0.4 예정).
pub fn call_tool(tool: &str, args_json: &str) -> Result<(), ToolError> {
    let tool_bytes = tool.as_bytes();
    let args_bytes = args_json.as_bytes();
    // SAFETY: 두 슬라이스 모두 호출 시점에 valid 한 wasm 메모리 영역을 가리키며,
    // 호스트는 두 입력을 즉시 호스트 측으로 복사한 뒤에야 정책 검증으로 넘어갑니다.
    let rc = unsafe {
        lumen_call_tool(
            tool_bytes.as_ptr() as i32,
            tool_bytes.len() as i32,
            args_bytes.as_ptr() as i32,
            args_bytes.len() as i32,
        )
    };
    match rc {
        0 => Ok(()),
        -1 => Err(ToolError::Denied),
        -2 => Err(ToolError::Memory),
        other => Err(ToolError::Unknown(other)),
    }
}

/// 동적 문자열 조립이 필요한 보조 함수들. `alloc` feature 가 켜진 경우만 컴파일.
#[cfg(feature = "alloc")]
pub mod alloc_helpers {
    use super::{call_tool, log, LogLevel, ToolError};
    use alloc::string::String;

    /// 정해진 형식으로 audit 한 줄을 남기는 헬퍼 - 디버깅용.
    pub fn announce(name: &str, version: &str) {
        let mut buf = String::with_capacity(name.len() + version.len() + 16);
        buf.push_str("agent=");
        buf.push_str(name);
        buf.push_str(" version=");
        buf.push_str(version);
        log(LogLevel::Info, &buf);
    }

    /// `Vec<u8>` 페이로드를 만들어 도구를 호출합니다. 파라미터를 런타임에
    /// 조립해야 하는 에이전트에서 유용합니다. 입력은 UTF-8 이라고 가정합니다.
    pub fn call_tool_owned(tool: &str, args_bytes: &[u8]) -> Result<(), ToolError> {
        let s = core::str::from_utf8(args_bytes).map_err(|_| ToolError::Memory)?;
        call_tool(tool, s)
    }
}
