//! `lumen defend` - stdin 또는 `--text` 인자에 대해 방어 엔진 실행.

use std::io::Read;

use clap::Args as ClapArgs;
use lumen_defense::DefenseEngine;

/// `defend` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// 인라인 프롬프트 (없으면 stdin 에서 읽음).
    #[arg(long)]
    pub text: Option<String>,
}

/// 실행.
pub fn run(args: Args) -> anyhow::Result<()> {
    let prompt = match args.text {
        Some(s) => s,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    let engine = DefenseEngine::new();
    let verdict = engine.analyze(&prompt);
    println!("verdict: {}", serde_json::to_string(&verdict)?);
    println!("corpus_version: {}", engine.corpus_version());
    Ok(())
}
