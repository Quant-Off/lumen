//! `lumen verify-engine` - 엔진 매니페스트의 서명과 파일 해시를 검증.

use std::path::{Path, PathBuf};

use clap::Args as ClapArgs;
use lumen_core::VerifyingKey;
use lumen_provenance::{verify_engine, EngineManifest};

/// `verify-engine` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 매니페스트 TOML 경로. 상대 경로는 이 파일의 디렉토리 기준으로 해석합니다.
    #[arg(long)]
    pub manifest: PathBuf,
    /// 신뢰 서명자 공개 키 (hex). 반복 가능. 하나라도 주면 미서명 매니페스트는 거부됩니다.
    #[arg(long = "trusted-signer")]
    pub trusted_signers: Vec<String>,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let manifest: EngineManifest = toml::from_str(&std::fs::read_to_string(&args.manifest)?)?;
    let signers = args
        .trusted_signers
        .iter()
        .map(|s| {
            let mut out = [0u8; 32];
            hex::decode_to_slice(s.trim(), &mut out)?;
            Ok(VerifyingKey::from_bytes(&out)?)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let base = args.manifest.parent().unwrap_or_else(|| Path::new("."));
    let v = verify_engine(base, &manifest, &signers)?;
    println!(
        "OK  {} v{}  hash={}  files={}  size={} bytes  signer={}",
        v.name,
        v.version,
        v.hash,
        v.files.len(),
        v.size_bytes,
        v.signer.map(|k| k.to_hex()).unwrap_or_else(|| "-".into())
    );
    Ok(())
}
