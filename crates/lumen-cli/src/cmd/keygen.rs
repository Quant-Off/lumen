//! `lumen keygen` - 매니페스트 서명용 Ed25519 키 파일 생성.

use std::path::PathBuf;

use clap::Args as ClapArgs;

/// `keygen` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 시드를 기록할 키 파일 경로. 기존 파일은 덮어쓰지 않습니다.
    #[arg(long)]
    pub out: PathBuf,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let key = super::keyfile::write_new_signing_key(&args.out)?;
    println!("key file:   {}", args.out.display());
    println!("public key: {}", key.verifying_key().to_hex());
    println!("add the public key to `trusted_signers` in the policy file");
    Ok(())
}
