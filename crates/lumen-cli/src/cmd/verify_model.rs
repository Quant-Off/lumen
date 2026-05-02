//! `lumen verify-model` - 모델 파일을 매니페스트와 비교 검증.

use std::path::PathBuf;

use clap::Args as ClapArgs;
use lumen_provenance::manifest::ModelManifest;
use lumen_provenance::verify_model;

/// `verify-model` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 매니페스트 TOML 경로.
    #[arg(long)]
    pub manifest: PathBuf,
    /// 모델 파일 경로. 생략 시 매니페스트의 `path` 사용.
    #[arg(long)]
    pub file: Option<PathBuf>,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let manifest_text = std::fs::read_to_string(&args.manifest)?;
    let manifest: ModelManifest = toml::from_str(&manifest_text)?;
    let file = args.file.unwrap_or_else(|| manifest.path.clone());
    let info = verify_model(&file, &manifest, &[])?;
    println!(
        "OK  {} v{}  format={:?}  hash={}  size={} bytes",
        info.name, info.version, info.format, info.hash, info.size_bytes
    );
    Ok(())
}
