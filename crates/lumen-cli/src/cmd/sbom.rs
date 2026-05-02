//! `lumen sbom` - 정책 파일의 모델로부터 `CycloneDX` SBOM 을 stdout 으로 발행.

use std::path::PathBuf;

use clap::Args as ClapArgs;

use crate::policy_file::PolicyFile;

/// `sbom` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 정책 TOML.
    #[arg(long)]
    pub policy: PathBuf,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let (policy, _hash) = PolicyFile::load(&args.policy)?;
    let doc = lumen_provenance::generate_sbom(&policy.models)?;
    println!("{}", serde_json::to_string_pretty(&doc)?);
    Ok(())
}
