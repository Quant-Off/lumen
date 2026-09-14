//! WASM 게스트에 노출되는 capability-gated 호스트 임포트.
//!
//! 모든 임포트는 게스트 선형 메모리에 대한 포인터/길이를 받습니다. 어떤 정책
//! 검사 *전에* 호스트 측에서 바이트를 WASM 메모리 밖 Vec 으로 복사하므로,
//! 게스트가 검사와 사용 사이에 버퍼를 변경할 수 없습니다.
//!
//! 임포트:
//! - `lumen_log(level: i32, ptr: i32, len: i32)`
//! - `lumen_call_tool(tool_ptr: i32, tool_len: i32, args_ptr: i32, args_len: i32) -> i32`
//!   * 성공 시 `0`, capability 거부 시 `-1`, 메모리 오류 시 `-2` 반환.
//!
//! `lumen_recv` 는 v0.4 자리표시자입니다 (호스트 측 큐에서 핸들로 도구 결과
//! 를 읽음).

use lumen_capability::{Action, Capability};
use lumen_core::{Error, Result, ToolId};
use wasmtime::{Caller, Linker, Memory};

use crate::host::{HostState, ToolCallRecord};

/// 단일 호스트 임포트 호출이 게스트 메모리에서 호스트 Vec 으로 복사할 수
/// 있는 최대 바이트 수 (64 KiB).
///
/// 이 한도가 없으면 게스트가 16 MiB 선형 메모리 페이지를 모두 채워
/// `lumen_log` / `lumen_call_tool` 인자로 반복 전달함으로써 호스트 측
/// 메모리를 과다 소비시키는 `DoS` 가 가능합니다. 도구 인자나 로그 메시지가
/// 정상 운영에서 64 KiB 를 넘는 경우는 거의 없으므로 보수적인 상한으로
/// 충분합니다.
const MAX_GUEST_BUFFER: usize = 64 * 1024;

/// 단일 실행에서 audit 버퍼가 누적할 수 있는 최대 항목 수.
///
/// 게스트가 짧은 메시지로 반복 `lumen_log` 를 호출해 호스트 audit Vec 을
/// 무한 증식시키지 못하도록 보호합니다. 한도 도달 후 모든 후속 `lumen_log`
/// 호출은 무시됩니다 (실행은 계속 — fail-safe).
const MAX_AUDIT_ENTRIES: usize = 4096;

/// 단일 실행에서 기록할 수 있는 최대 도구 호출 시도 수.
const MAX_TOOL_CALLS: usize = 1024;

/// 모듈 이름 `lumen` 아래 Lumen 호스트 임포트를 등록합니다.
///
/// `caps` 콜백은 도구 호출 시도에 적용 가능한 capability 토큰을 반환합니다 -
/// 정책 엔진은 그것을 검증합니다. capability 를 클로저로 넘기면 호스트가
/// linker 를 재구성하지 않고도 step 사이에 capability 를 회전할 수 있습니다.
pub fn register<F>(linker: &mut Linker<HostState>, caps: F) -> Result<()>
where
    F: Fn(&str) -> Option<Capability> + Send + Sync + 'static,
{
    let caps = std::sync::Arc::new(caps);

    linker
        .func_wrap(
            "lumen",
            "lumen_log",
            |mut caller: Caller<'_, HostState>, level: i32, ptr: i32, len: i32| {
                let Some(memory) = find_memory(&mut caller) else {
                    return;
                };
                let Ok(b) = read_guest_bytes(&mut caller, &memory, ptr, len) else {
                    return;
                };
                let msg = String::from_utf8_lossy(&b).into_owned();
                let line = format!("[guest level={level}] {msg}");
                tracing::info!(target: "lumen.sandbox", "{line}");
                let mut audit = caller.data().audit.lock();
                if audit.len() < MAX_AUDIT_ENTRIES {
                    audit.push(line);
                }
                // 한도 도달 시 silently 드롭 - tracing 은 이미 발행되었고,
                // panic 또는 trap 으로 호스트를 흔들지 않습니다.
            },
        )
        .map_err(|e| Error::Sandbox(format!("register lumen_log: {e}")))?;

    let caps_for_call = caps.clone();
    linker
        .func_wrap(
            "lumen",
            "lumen_call_tool",
            move |mut caller: Caller<'_, HostState>,
                  tool_ptr: i32,
                  tool_len: i32,
                  args_ptr: i32,
                  args_len: i32|
                  -> i32 {
                let Some(memory) = find_memory(&mut caller) else {
                    return -2;
                };
                let Ok(tool_bytes) = read_guest_bytes(&mut caller, &memory, tool_ptr, tool_len)
                else {
                    return -2;
                };
                let Ok(_args_bytes) = read_guest_bytes(&mut caller, &memory, args_ptr, args_len)
                else {
                    return -2;
                };
                let Ok(tool_str) = std::str::from_utf8(&tool_bytes) else {
                    return -2;
                };
                let Ok(tool_id) = ToolId::new(tool_str) else {
                    return -1;
                };
                let Some(cap) = caps_for_call(tool_str) else {
                    record(&caller, tool_str, false);
                    return -1;
                };
                let state = caller.data();
                let res =
                    state
                        .policy
                        .check(&cap, &state.agent, &Action::CallTool(&tool_id), state.now);
                let ok = res.is_ok();
                record(&caller, tool_str, ok);
                if ok {
                    0
                } else {
                    -1
                }
            },
        )
        .map_err(|e| Error::Sandbox(format!("register lumen_call_tool: {e}")))?;

    Ok(())
}

fn find_memory(caller: &mut Caller<'_, HostState>) -> Option<Memory> {
    caller
        .get_export("memory")
        .and_then(wasmtime::Extern::into_memory)
}

fn read_guest_bytes(
    caller: &mut Caller<'_, HostState>,
    memory: &Memory,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>> {
    let start = usize::try_from(ptr).map_err(|_| Error::Sandbox("negative ptr".into()))?;
    let n = usize::try_from(len).map_err(|_| Error::Sandbox("negative len".into()))?;
    if n > MAX_GUEST_BUFFER {
        return Err(Error::Sandbox(format!(
            "guest buffer too large: {n} > {MAX_GUEST_BUFFER}"
        )));
    }
    let data = memory.data(&*caller);
    let end = start
        .checked_add(n)
        .ok_or_else(|| Error::Sandbox("ptr/len overflow".into()))?;
    if end > data.len() {
        return Err(Error::Sandbox("ptr/len out of bounds".into()));
    }
    Ok(data[start..end].to_vec())
}

fn record(caller: &Caller<'_, HostState>, tool: &str, authorised: bool) {
    let mut tool_calls = caller.data().tool_calls.lock();
    if tool_calls.len() < MAX_TOOL_CALLS {
        tool_calls.push(ToolCallRecord {
            tool: tool.to_string(),
            authorised,
        });
    }
    // 한도 도달 시 드롭 - 이미 audit::deny / allow 가 tracing 으로 기록.
}
