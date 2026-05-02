//! 실제 Rust -> wasm32 로 컴파일된 `lumen-echo-agent` 를 샌드박스로 로드해
//! capability 게이팅이 동작하는지 끝-끝으로 검증합니다.
//!
//! 호스트 측은 `wasm32-unknown-unknown` 타겟으로 `agents/echo-agent` 를
//! 미리 컴파일했다고 가정합니다. CI 와 로컬 모두에서 이 테스트를 실행하기
//! 전에 다음 명령이 한 번 수행되어야 합니다.
//!
//! ```sh
//! cd agents/echo-agent && cargo build --release --target wasm32-unknown-unknown
//! ```
//!
//! 산출물이 없으면 테스트는 *skip* 처리됩니다 (CI 매트릭스에 wasm32 타겟이
//! 항상 설치돼 있다고 가정하지 않습니다).

use std::path::PathBuf;
use std::sync::Arc;

use lumen_capability::{
    capability::{Capability, CapabilityBody},
    PolicyEngine, Resource,
};
use lumen_core::{AgentId, CapabilityId, SigningKey, Timestamp, ToolId};
use lumen_sandbox::{HostState, Sandbox, SandboxConfig};
use rand::rngs::OsRng;

/// 워크스페이스 루트 기준 wasm 산출물 경로.
fn echo_agent_wasm_path() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/lumen-sandbox
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    workspace_root.join("agents/echo-agent/target/wasm32-unknown-unknown/release/lumen_echo_agent.wasm")
}

#[tokio::test]
async fn echo_agent_no_capability_denied() {
    let path = echo_agent_wasm_path();
    if !path.exists() {
        eprintln!(
            "skipping: {path:?} not built - run `cd agents/echo-agent && cargo build --release --target wasm32-unknown-unknown`"
        );
        return;
    }
    let wasm = std::fs::read(&path).expect("read wasm");

    let policy = Arc::new(PolicyEngine::new(vec![]));
    let agent = AgentId::random(&mut OsRng);
    let host = HostState::new(policy, agent).with_now(Timestamp::from_millis(0));

    let sandbox = Sandbox::new(SandboxConfig::default()).expect("sandbox");
    // capability 미제공 -> 모든 tool call 은 -1 로 거부됨.
    let after = sandbox
        .run_module(&wasm, host, |_tool| None)
        .await
        .expect("run");

    let calls = after.tool_calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "echo");
    assert!(!calls[0].authorised, "capability 없이는 통과해선 안 됩니다");

    let audit = after.audit.lock();
    // audit 에는 'starting', 'denied', 'done' 세 줄이 모두 들어 있어야 합니다.
    let joined: String = audit.join(" | ");
    assert!(
        joined.contains("starting"),
        "starting audit missing: {audit:?}"
    );
    assert!(
        joined.contains("denied") || joined.contains("Denied") || joined.contains("denied"),
        "denied audit missing: {audit:?}"
    );
    assert!(joined.contains("done"), "done audit missing: {audit:?}");
}

#[tokio::test]
async fn echo_agent_with_capability_authorised() {
    let path = echo_agent_wasm_path();
    if !path.exists() {
        eprintln!("skipping: wasm not built - see prior test");
        return;
    }
    let wasm = std::fs::read(&path).expect("read wasm");

    let sk = SigningKey::generate(&mut OsRng);
    let vk = sk.verifying_key();
    let agent = AgentId::random(&mut OsRng);
    let policy = Arc::new(PolicyEngine::new(vec![vk]));
    let cap = Capability::sign(
        CapabilityBody {
            id: CapabilityId::random(&mut OsRng),
            audience: agent,
            resource: Resource::Tool(ToolId::new("echo").unwrap()),
            nonce: [0xAA; 16],
            expires_at: Timestamp::FOREVER,
            issuer: agent,
        },
        &sk,
    )
    .unwrap();
    let host = HostState::new(policy, agent).with_now(Timestamp::from_millis(0));

    let sandbox = Sandbox::new(SandboxConfig::default()).expect("sandbox");
    let after = sandbox
        .run_module(&wasm, host, move |tool| {
            if tool == "echo" {
                Some(cap.clone())
            } else {
                None
            }
        })
        .await
        .expect("run");

    let calls = after.tool_calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "echo");
    assert!(calls[0].authorised, "capability 가 있는 호출은 통과해야 합니다");

    let audit = after.audit.lock();
    let joined: String = audit.join(" | ");
    assert!(joined.contains("ok"), "ok audit missing: {audit:?}");
}
