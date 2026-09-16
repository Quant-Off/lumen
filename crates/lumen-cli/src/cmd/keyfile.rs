//! Ed25519 서명 키 파일 입출력.
//!
//! 키 파일은 32 바이트 시드의 hex 한 줄입니다. 생성 시 `0600` 으로 만들고 기존
//! 파일은 덮어쓰지 않으며, 읽을 때는 그룹 / 타인 권한이 있으면 거부합니다.

use std::path::Path;

use lumen_core::SigningKey;
use zeroize::Zeroizing;

/// 키 파일을 읽어 서명 키를 복원합니다.
///
/// # Errors
/// 파일 권한이 `0600` 보다 넓거나 내용이 64 hex 자가 아니면 실패.
pub fn read_signing_key(path: &Path) -> anyhow::Result<SigningKey> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        anyhow::bail!("key file {path:?} is not a regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            anyhow::bail!("key file {path:?} has mode {mode:o}; expected 0600");
        }
    }
    let raw = Zeroizing::new(std::fs::read_to_string(path)?);
    Ok(SigningKey::from_seed_hex(&raw)?)
}

/// 새 키를 생성해 `path` 에 `0600` 으로 기록합니다. 기존 파일은 덮어쓰지 않습니다.
pub fn write_new_signing_key(path: &Path) -> anyhow::Result<SigningKey> {
    use std::io::Write;

    let key = SigningKey::generate(&mut lumen_core::OsRng);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| anyhow::anyhow!("key file {path:?}: {e}"))?;
    let seed = key.seed_hex();
    file.write_all(seed.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(key)
}
