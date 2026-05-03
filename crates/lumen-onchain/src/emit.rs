//! Verifier scaffold 산출물을 디스크에 출력.
//!
//! [`emit_artifacts`] 가 단일 디렉토리 아래에 결정론적 layout 으로 source +
//! deploy script 를 작성합니다. 같은 [`VerifierMeta`] 입력에 대해 byte-동일한
//! 출력이 보장되어, ZK pipeline 의 다른 부분 (witness, proof) 처럼 재현
//! 가능합니다.

use std::fs;
use std::path::{Path, PathBuf};

use lumen_core::{Error, Result};

use crate::{evm, mina, Chain, VerifierMeta};

/// `emit_artifacts` 가 출력하는 파일 목록.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmittedArtifacts {
    /// 출력 디렉토리 (호출자가 전달한 그대로).
    pub out_dir: PathBuf,
    /// 작성된 파일들의 절대 경로.
    pub files: Vec<PathBuf>,
}

/// `chain` 에 맞는 verifier 산출물을 `out_dir` 아래에 작성합니다.
///
/// 디렉토리는 없으면 생성합니다. 기존 파일은 *덮어쓰기* 합니다 (재현 빌드).
pub fn emit_artifacts(chain: Chain, meta: &VerifierMeta, out_dir: &Path) -> Result<EmittedArtifacts> {
    fs::create_dir_all(out_dir)?;
    let mut files = Vec::new();

    match chain {
        Chain::Evm => {
            let src_dir = out_dir.join("src");
            fs::create_dir_all(&src_dir)?;

            let sol_path = src_dir.join("RoutingVerifier.sol");
            write_file(&sol_path, &evm::solidity_source(meta))?;
            files.push(sol_path);

            let toml_path = out_dir.join("foundry.toml");
            write_file(&toml_path, &evm::foundry_toml())?;
            files.push(toml_path);

            let deploy_path = out_dir.join("deploy.sh");
            write_file(&deploy_path, &evm::forge_deploy_script(meta))?;
            files.push(deploy_path);

            #[cfg(unix)]
            mark_executable(&out_dir.join("deploy.sh"))?;
        }
        Chain::Mina => {
            let src_dir = out_dir.join("src");
            fs::create_dir_all(&src_dir)?;

            let ts_path = src_dir.join("RoutingVerifier.ts");
            write_file(&ts_path, &mina::o1js_source(meta))?;
            files.push(ts_path);

            let deploy_path = out_dir.join("deploy.sh");
            write_file(&deploy_path, &mina::mina_deploy_script(meta))?;
            files.push(deploy_path);

            #[cfg(unix)]
            mark_executable(&out_dir.join("deploy.sh"))?;
        }
    }

    let meta_path = out_dir.join("verifier.meta.json");
    let meta_json = serde_json::to_string_pretty(meta)
        .map_err(|e| Error::Decode(format!("verifier meta encode: {e}")))?;
    write_file(&meta_path, &meta_json)?;
    files.push(meta_path);

    Ok(EmittedArtifacts {
        out_dir: out_dir.to_path_buf(),
        files,
    })
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).map_err(Error::from)
}

#[cfg(unix)]
fn mark_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_emit_writes_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        let meta = VerifierMeta::for_circuit("lumen.routing.binary.v1");
        let out = emit_artifacts(Chain::Evm, &meta, dir.path()).unwrap();
        assert!(out.files.iter().any(|p| p.ends_with("RoutingVerifier.sol")));
        assert!(out.files.iter().any(|p| p.ends_with("foundry.toml")));
        assert!(out.files.iter().any(|p| p.ends_with("deploy.sh")));
        assert!(out.files.iter().any(|p| p.ends_with("verifier.meta.json")));
    }

    #[test]
    fn mina_emit_writes_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        let meta = VerifierMeta::for_circuit("lumen.routing.binary.v1");
        let out = emit_artifacts(Chain::Mina, &meta, dir.path()).unwrap();
        assert!(out.files.iter().any(|p| p.ends_with("RoutingVerifier.ts")));
        assert!(out.files.iter().any(|p| p.ends_with("deploy.sh")));
        assert!(out.files.iter().any(|p| p.ends_with("verifier.meta.json")));
    }

    #[test]
    fn emit_is_idempotent_and_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let meta = VerifierMeta::for_circuit("lumen.routing.binary.v1");
        let out1 = emit_artifacts(Chain::Evm, &meta, dir.path()).unwrap();
        let out2 = emit_artifacts(Chain::Evm, &meta, dir.path()).unwrap();
        assert_eq!(out1.files, out2.files);
        // 파일 내용도 byte-동일해야.
        for p in &out1.files {
            let a = std::fs::read(p).unwrap();
            // 재emit 후 다시 읽어도 동일.
            let _ = std::fs::read(p).unwrap();
            // 재차 emit 후 비교.
            let _ = emit_artifacts(Chain::Evm, &meta, dir.path()).unwrap();
            let b = std::fs::read(p).unwrap();
            assert_eq!(a, b, "deterministic emit at {p:?}");
        }
    }
}
