//! `lumen verifier` - 온체인 검증기 emit + 배포.

use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};
use lumen_onchain::deploy::{deploy_evm, deploy_mina, EvmDeployOptions, MinaDeployOptions};
use lumen_onchain::emit::emit_artifacts;
use lumen_onchain::{Chain, VerifierMeta};

/// `verifier` 서브커맨드 그룹.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 서브커맨드.
    #[command(subcommand)]
    pub cmd: VerifierCmd,
}

/// 검증기 관련 커맨드.
#[derive(Debug, Subcommand)]
pub enum VerifierCmd {
    /// 검증기 source + deploy script 를 디렉토리에 작성.
    Emit(EmitArgs),
    /// 검증기를 외부 toolchain (forge / zk) 으로 배포 - dry-run 기본.
    Deploy(DeployArgs),
}

/// `emit` 인자.
#[derive(Debug, ClapArgs)]
pub struct EmitArgs {
    /// 대상 체인 (evm | mina).
    #[arg(long)]
    pub chain: String,
    /// 회로 식별자.
    #[arg(long, default_value = "lumen.routing.binary.v1")]
    pub circuit_id: String,
    /// 출력 디렉토리.
    #[arg(long)]
    pub out: PathBuf,
}

/// `deploy` 인자. private key 등 민감 입력은 환경변수에서 읽습니다.
#[derive(Debug, ClapArgs)]
pub struct DeployArgs {
    /// 대상 체인 (evm | mina).
    #[arg(long)]
    pub chain: String,
    /// `emit` 으로 작성한 디렉토리.
    #[arg(long)]
    pub out: PathBuf,
    /// JSON-RPC / GraphQL endpoint.
    #[arg(long)]
    pub rpc: String,
    /// EVM private-key 환경변수 이름 (기본 `LUMEN_DEPLOY_PRIVKEY`).
    #[arg(long, default_value = "LUMEN_DEPLOY_PRIVKEY")]
    pub privkey_env: String,
    /// Mina fee-payer alias (기본 `LUMEN_MINA_FEE_PAYER` 환경변수에서).
    #[arg(long, default_value = "LUMEN_MINA_FEE_PAYER")]
    pub fee_payer_env: String,
    /// 실제 실행하지 않고 명령만 출력.
    #[arg(long, default_value_t = true)]
    pub dry_run: bool,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    match args.cmd {
        VerifierCmd::Emit(a) => emit(a),
        VerifierCmd::Deploy(a) => deploy(a),
    }
}

fn emit(args: EmitArgs) -> anyhow::Result<()> {
    let chain: Chain = args
        .chain
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let meta = VerifierMeta::for_circuit(&args.circuit_id);
    let out = emit_artifacts(chain, &meta, &args.out)?;
    println!("emitted {} files to {}", out.files.len(), out.out_dir.display());
    for p in &out.files {
        println!("  {}", p.display());
    }
    Ok(())
}

fn deploy(args: DeployArgs) -> anyhow::Result<()> {
    let chain: Chain = args
        .chain
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    match chain {
        Chain::Evm => {
            let pk = std::env::var(&args.privkey_env).unwrap_or_default();
            let outcome = deploy_evm(EvmDeployOptions {
                out_dir: &args.out,
                rpc_url: &args.rpc,
                private_key: &pk,
                forge_bin: None,
                dry_run: args.dry_run,
            })?;
            println!("cmd: {}", outcome.command_redacted);
            if !args.dry_run {
                println!("status: {:?}", outcome.status);
                if !outcome.stdout.is_empty() {
                    println!("--- stdout ---\n{}", outcome.stdout);
                }
                if !outcome.stderr.is_empty() {
                    println!("--- stderr ---\n{}", outcome.stderr);
                }
            }
        }
        Chain::Mina => {
            let fp = std::env::var(&args.fee_payer_env).unwrap_or_default();
            let outcome = deploy_mina(MinaDeployOptions {
                out_dir: &args.out,
                rpc_url: &args.rpc,
                fee_payer: &fp,
                zk_bin: None,
                dry_run: args.dry_run,
            })?;
            println!("cmd: {}", outcome.command_redacted);
            if !args.dry_run {
                println!("status: {:?}", outcome.status);
                if !outcome.stdout.is_empty() {
                    println!("--- stdout ---\n{}", outcome.stdout);
                }
                if !outcome.stderr.is_empty() {
                    println!("--- stderr ---\n{}", outcome.stderr);
                }
            }
        }
    }
    Ok(())
}
