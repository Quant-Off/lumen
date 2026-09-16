//! `lumen sign-model` - 모델 매니페스트의 해시를 파일과 대조한 뒤 서명.

use std::path::{Path, PathBuf};

use clap::Args as ClapArgs;
use lumen_provenance::{verify_model, ModelManifest};

/// `sign-model` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 서명할 매니페스트 TOML. `hash` 가 채워져 있어야 합니다.
    #[arg(long)]
    pub manifest: PathBuf,
    /// 모델 파일 경로. 생략 시 매니페스트의 `path` (매니페스트 디렉토리 기준).
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// `lumen keygen` 으로 만든 서명 키 파일 (0600).
    #[arg(long)]
    pub key: PathBuf,
    /// 서명된 매니페스트를 기록할 경로. 생략 시 stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// 실행.
///
/// 서명 전에 파일 해시를 매니페스트와 대조하므로 존재하지 않거나 변조된
/// 가중치에 서명하는 일이 없습니다.
pub fn run(args: Args) -> anyhow::Result<()> {
    let key = super::keyfile::read_signing_key(&args.key)?;
    let mut manifest: ModelManifest = toml::from_str(&std::fs::read_to_string(&args.manifest)?)?;
    let base = args.manifest.parent().unwrap_or_else(|| Path::new("."));
    let file = args.file.unwrap_or_else(|| {
        if manifest.path.is_absolute() {
            manifest.path.clone()
        } else {
            base.join(&manifest.path)
        }
    });
    let info = verify_model(&file, &manifest, &[])?;
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
        "model {} v{}  hash={}  size={} bytes  signer={}",
        info.name,
        info.version,
        info.hash,
        info.size_bytes,
        key.verifying_key().to_hex()
    );
    Ok(())
}
