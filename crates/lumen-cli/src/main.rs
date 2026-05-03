//! `lumen` CLI 진입점.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// 바이너리 전용 크레이트 - 모든 public 항목이 어차피 내부용입니다.
#![allow(unreachable_pub)]

mod cmd;
mod policy_file;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "lumen",
    version,
    about = "Lumen - 보안 검증 가능 AI 에이전트 런타임"
)]
struct Cli {
    /// 서브커맨드.
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// 샘플 정책 TOML 을 stdout 으로 출력합니다.
    Init(cmd::init::Args),
    /// 모델 파일을 매니페스트와 비교 검증합니다.
    VerifyModel(cmd::verify_model::Args),
    /// 정책 파일의 매니페스트로부터 CycloneDX SBOM 을 발행합니다.
    Sbom(cmd::sbom::Args),
    /// 방어 엔진으로 프롬프트를 분석합니다.
    Defend(cmd::defend::Args),
    /// 고정 샘플에 대해 mock 라우팅 증명을 생성하고 검증합니다.
    Prove(cmd::prove::Args),
    /// 정책 해시 핀을 강제하면서 한 번의 에이전트 step 을 끝-끝으로 실행.
    Run(cmd::run::Args),
    /// 온체인 검증기 (EVM / Mina) emit 및 배포.
    Verifier(cmd::verifier::Args),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,lumen=info")),
        )
        .with_target(true)
        .compact()
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init(a) => cmd::init::run(a),
        Cmd::VerifyModel(a) => cmd::verify_model::run(a),
        Cmd::Sbom(a) => cmd::sbom::run(a),
        Cmd::Defend(a) => cmd::defend::run(a),
        Cmd::Prove(a) => cmd::prove::run(a),
        Cmd::Run(a) => cmd::run::run(a).await,
        Cmd::Verifier(a) => cmd::verifier::run(a),
    }
}
