//! `lumen run` - 정책 파일로부터 단일 에이전트 step 실행.
//!
//! 정책 파일의 BLAKE3 가 `--policy-hash` 와 일치해야 하며, 핀이 없으면 명령은
//! 실행을 거부합니다. 이는 Lumen 의 제로 트러스트 에토스의 가장 작은 UX 차원
//! 시연: 런타임은 절대 디스크 내용을 신뢰하지 않고, 사전 commitment 된
//! 해시만 신뢰합니다.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Args as ClapArgs;
use lumen_agent::tool::{AddTool, EchoTool, Tool};
use lumen_agent::AgentRuntime;
use lumen_capability::PolicyEngine;
use lumen_core::{Blake3Hash, ToolId};
use lumen_inference::DummyEngine;
use lumen_orchestrator::{AgentSpec, Orchestrator};
use lumen_zkml::mock::MockVk;

use crate::policy_file::PolicyFile;

/// `run` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 정책 TOML 경로.
    #[arg(long)]
    pub policy: PathBuf,
    /// 정책 파일의 핀된 BLAKE3 hex (필수).
    #[arg(long)]
    pub policy_hash: String,
    /// 단일 step 의 프롬프트.
    #[arg(long)]
    pub prompt: String,
}

/// 실행.
pub async fn run(args: Args) -> anyhow::Result<()> {
    // 1. 정책 파일 로드 및 핀 검증.
    let pinned: Blake3Hash = args.policy_hash.parse()?;
    let (policy, actual) = PolicyFile::load(&args.policy)?;
    if pinned != actual {
        anyhow::bail!("policy hash mismatch: pinned={pinned} actual={actual}");
    }
    println!("policy hash verified: {actual}");

    // 2. 참조된 모델 매니페스트 검증.
    let policy_dir = args
        .policy
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    for manifest in &policy.models {
        let path = PolicyFile::resolve_model_path(policy_dir, manifest);
        match lumen_provenance::verify_model(&path, manifest, &[]) {
            Ok(info) => println!(
                "model verified: {} v{} ({:?}, {} bytes)",
                info.name, info.version, info.format, info.size_bytes
            ),
            Err(e) => anyhow::bail!("model verification failed for {}: {e}", manifest.name),
        }
    }

    // 3. 런타임 빌드.
    let policy_engine = Arc::new(PolicyEngine::new(policy.trusted_issuers.clone()));
    let policy_hash = actual; // 라우팅 결정에 바인드
    let echo_id = ToolId::new("echo")?;
    let add_id = ToolId::new("add")?;

    let mut builder = AgentRuntime::builder(policy.agent.id, policy_engine.clone())
        .inference(Arc::new(DummyEngine::new()))
        .tool(Tool {
            id: echo_id.clone(),
            schema: serde_json::json!({}),
            handler: Arc::new(EchoTool),
        })?
        .tool(Tool {
            id: add_id.clone(),
            schema: serde_json::json!({}),
            handler: Arc::new(AddTool),
        })?
        .proving_vk(MockVk {
            circuit_id: "lumen.routing.v1".into(),
        })
        .policy_hash(policy_hash);

    // 정책 파일이 echo / add 에 대해 선언한 capability 를 wiring.
    for cap in policy.capabilities.iter().cloned() {
        if let lumen_capability::Resource::Tool(tool_id) = cap.body.resource.clone() {
            builder = builder.capability(tool_id, cap);
        }
    }

    let runtime = Arc::new(builder.build()?);

    // 4. 오케스트레이터 spawn 후 step 한 번 실행.
    let orchestrator = Orchestrator::new();
    let handle = orchestrator.spawn(AgentSpec {
        agent_id: policy.agent.id,
        runtime,
    })?;

    let result = handle.step(&args.prompt).await?;
    println!("\n--- step result ---");
    println!("defense:      {:?}", result.defense_verdict);
    println!("completion:   {:?}", result.completion);
    if let Some(out) = &result.tool_output {
        println!("tool output:  {out}");
    }
    println!(
        "routing tool: {:?}",
        result.routing.public.tool.as_ref().map(|t| t.as_str())
    );
    println!("prompt hash:  {}", result.routing.public.prompt_hash);
    println!("policy hash:  {}", result.routing.public.policy_hash);
    println!("proof digest: {}", result.proof.digest);
    println!("verification: {:?}", result.verification);

    drop(handle);
    orchestrator.shutdown_all().await?;
    Ok(())
}
