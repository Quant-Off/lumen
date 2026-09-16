//! `lumen sign-engine` - 엔진 실행 파일과 부속 파일을 해시하고 서명한 매니페스트 발행.

use std::path::PathBuf;

use clap::Args as ClapArgs;
use lumen_provenance::EngineManifest;

/// `sign-engine` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 엔진 이름 (예: `llama-server`).
    #[arg(long)]
    pub name: String,
    /// 엔진 버전 (빌드 번호 권장).
    #[arg(long)]
    pub version: String,
    /// 실행 파일 경로.
    #[arg(long)]
    pub binary: PathBuf,
    /// 실행 파일이 로드하는 부속 파일 (공유 라이브러리 등). 반복 가능.
    #[arg(long = "file")]
    pub files: Vec<PathBuf>,
    /// 옵션 SPDX 라이선스 식별자.
    #[arg(long)]
    pub license: Option<String>,
    /// `lumen keygen` 으로 만든 서명 키 파일 (0600).
    #[arg(long)]
    pub key: PathBuf,
    /// 서명된 매니페스트 TOML 을 기록할 경로. 생략 시 stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let key = super::keyfile::read_signing_key(&args.key)?;
    let mut manifest = EngineManifest::pin(args.name, args.version, args.binary, &args.files)?;
    manifest.license = args.license;
    manifest.sign_with(&key)?;
    let text = toml::to_string(&manifest)?;
    match &args.out {
        Some(p) => {
            std::fs::write(p, &text)?;
            eprintln!("signed manifest written to {}", p.display());
        }
        None => print!("{text}"),
    }
    eprintln!(
        "engine {} v{}  hash={}  files={}  signer={}",
        manifest.name,
        manifest.version,
        manifest.hash,
        manifest.files.len(),
        key.verifying_key().to_hex()
    );
    Ok(())
}
