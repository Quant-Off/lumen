//! `lumen prove` - 고정 샘플에 대해 mock 라우팅 증명을 생성하고 검증.

use clap::Args as ClapArgs;
use lumen_core::{Blake3Hash, ToolId};
use lumen_zkml::mock::{for_types, verify_with_witness, MockVk};
use lumen_zkml::{ProvingSystem, Verification};

/// `prove` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 커밋할 프롬프트 (BLAKE3 가 공개 입력으로 사용됨).
    #[arg(long, default_value = "echo hello")]
    pub prompt: String,
    /// 라우팅 결정에 바인드할 도구 id.
    #[arg(long, default_value = "echo")]
    pub tool: String,
    /// 회로 id 라벨.
    #[arg(long, default_value = "lumen.routing.v1")]
    pub circuit_id: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Public {
    prompt_hash: Blake3Hash,
    tool: Option<ToolId>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Witness {
    args_json: String,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let public = Public {
        prompt_hash: Blake3Hash::of(args.prompt.as_bytes()),
        tool: Some(ToolId::new(&args.tool)?),
    };
    let witness = Witness {
        args_json: serde_json::json!({ "text": &args.prompt }).to_string(),
    };
    let prover = for_types::<Witness, Public>();
    let vk = MockVk {
        circuit_id: args.circuit_id.clone(),
    };
    let proof = prover.prove(&vk, &public, &witness)?;
    let v = verify_with_witness(&vk, &public, &witness, &proof)?;

    println!("circuit_id   : {}", args.circuit_id);
    println!("prompt_hash  : {}", public.prompt_hash);
    println!("proof_digest : {}", proof.digest);
    println!("verification : {v:?}");
    if v != Verification::CommitmentOnly {
        anyhow::bail!("verification did not return CommitmentOnly");
    }
    Ok(())
}
