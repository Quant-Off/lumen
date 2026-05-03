//! 외부 toolchain 을 자식 프로세스로 호출하는 배포 wrapper.
//!
//! 본 모듈은 의도적으로 *얇습니다*: 보안 결정 (서명, 회로 식별자, ABI) 은
//! 모두 [`crate::emit`] 가 emit 한 source 안에 박혀 있고, 여기서는 빌드/배포
//! 도구를 호출하기만 합니다. dry-run 모드에서는 명령을 출력만 하므로
//! 에어갭 환경에서 안전하게 inspect 한 뒤 텍스트로 운반할 수 있습니다.

use std::path::Path;
use std::process::{Command, Output};

use lumen_core::{Error, Result};

/// 배포 결과.
#[derive(Clone, Debug)]
pub struct DeployOutcome {
    /// 실행한 명령 (안전한 audit 용 - 비밀은 `***` 로 마스크됨).
    pub command_redacted: String,
    /// 자식 프로세스 stdout (UTF-8 lossy).
    pub stdout: String,
    /// 자식 프로세스 stderr (UTF-8 lossy).
    pub stderr: String,
    /// 종료 코드 (없으면 시그널로 종료).
    pub status: Option<i32>,
}

/// EVM 배포 옵션.
#[derive(Clone, Debug)]
pub struct EvmDeployOptions<'a> {
    /// `emit` 출력 디렉토리 (`foundry.toml` + `src/RoutingVerifier.sol`).
    pub out_dir: &'a Path,
    /// JSON-RPC URL.
    pub rpc_url: &'a str,
    /// EOA private key (hex). 명령 실행 시에만 사용되며 `command_redacted`
    /// 에는 마스크되어 들어갑니다.
    pub private_key: &'a str,
    /// `forge` 바이너리 경로 (기본 `forge`).
    pub forge_bin: Option<&'a str>,
    /// dry-run 이면 실제로 실행하지 않고 명령 문자열만 반환합니다.
    pub dry_run: bool,
}

/// EVM 배포를 실행합니다.
///
/// 외부 의존: `forge` (Foundry). 미설치 시 [`Error::NotImplemented`] 와 같은
/// 명시적 에러로 실패하므로 silent fallback 은 없습니다.
pub fn deploy_evm(opts: EvmDeployOptions<'_>) -> Result<DeployOutcome> {
    let forge = opts.forge_bin.unwrap_or("forge");
    let cmd_redacted = format!(
        "{forge} create --rpc-url {rpc} --private-key *** --broadcast src/RoutingVerifier.sol:RoutingVerifier",
        forge = forge,
        rpc = opts.rpc_url,
    );

    if opts.dry_run {
        tracing::info!(target: "lumen.onchain", cmd=%cmd_redacted, "dry-run evm deploy");
        return Ok(DeployOutcome {
            command_redacted: cmd_redacted,
            stdout: String::new(),
            stderr: String::new(),
            status: Some(0),
        });
    }

    let output: Output = Command::new(forge)
        .current_dir(opts.out_dir)
        .arg("create")
        .arg("--rpc-url")
        .arg(opts.rpc_url)
        .arg("--private-key")
        .arg(opts.private_key)
        .arg("--broadcast")
        .arg("src/RoutingVerifier.sol:RoutingVerifier")
        .output()
        .map_err(|e| Error::Invalid(format!("forge invocation: {e}")))?;

    Ok(DeployOutcome {
        command_redacted: cmd_redacted,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code(),
    })
}

/// Mina 배포 옵션.
#[derive(Clone, Debug)]
pub struct MinaDeployOptions<'a> {
    /// `emit` 출력 디렉토리.
    pub out_dir: &'a Path,
    /// Mina GraphQL endpoint.
    pub rpc_url: &'a str,
    /// `zk config` 에 등록된 fee payer alias.
    pub fee_payer: &'a str,
    /// `zk` 바이너리 경로 (기본 `zk`).
    pub zk_bin: Option<&'a str>,
    /// dry-run 이면 실제로 실행하지 않고 명령 문자열만 반환합니다.
    pub dry_run: bool,
}

/// Mina 배포를 실행합니다.
pub fn deploy_mina(opts: MinaDeployOptions<'_>) -> Result<DeployOutcome> {
    let zk = opts.zk_bin.unwrap_or("zk");
    let cmd_redacted = format!(
        "{zk} deploy --network {rpc} --fee-payer {fp}",
        zk = zk,
        rpc = opts.rpc_url,
        fp = opts.fee_payer,
    );

    if opts.dry_run {
        tracing::info!(target: "lumen.onchain", cmd=%cmd_redacted, "dry-run mina deploy");
        return Ok(DeployOutcome {
            command_redacted: cmd_redacted,
            stdout: String::new(),
            stderr: String::new(),
            status: Some(0),
        });
    }

    let output = Command::new(zk)
        .current_dir(opts.out_dir)
        .arg("deploy")
        .arg("--network")
        .arg(opts.rpc_url)
        .arg("--fee-payer")
        .arg(opts.fee_payer)
        .output()
        .map_err(|e| Error::Invalid(format!("zk invocation: {e}")))?;

    Ok(DeployOutcome {
        command_redacted: cmd_redacted,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_dry_run_does_not_invoke_external_tool() {
        let dir = tempfile::tempdir().unwrap();
        let res = deploy_evm(EvmDeployOptions {
            out_dir: dir.path(),
            rpc_url: "https://example.invalid",
            private_key: "0xdeadbeef",
            forge_bin: Some("/nonexistent/forge-binary"),
            dry_run: true,
        })
        .unwrap();
        assert_eq!(res.status, Some(0));
        assert!(res.command_redacted.contains("***"));
        assert!(!res.command_redacted.contains("0xdeadbeef"));
    }

    #[test]
    fn mina_dry_run_emits_redacted_command() {
        let dir = tempfile::tempdir().unwrap();
        let res = deploy_mina(MinaDeployOptions {
            out_dir: dir.path(),
            rpc_url: "https://example.invalid/graphql",
            fee_payer: "alice",
            zk_bin: Some("/nonexistent/zk"),
            dry_run: true,
        })
        .unwrap();
        assert_eq!(res.status, Some(0));
        assert!(res.command_redacted.contains("zk deploy"));
        assert!(res.command_redacted.contains("alice"));
    }

    #[test]
    fn evm_invocation_with_missing_binary_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = deploy_evm(EvmDeployOptions {
            out_dir: dir.path(),
            rpc_url: "https://example.invalid",
            private_key: "0xdeadbeef",
            forge_bin: Some("/nonexistent/forge-binary"),
            dry_run: false,
        })
        .unwrap_err();
        match err {
            Error::Invalid(_) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }
}
