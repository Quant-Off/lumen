//! GGUF 파일 헤더 구조 검증.
//!
//! GGUF 파일은 4 바이트 매직 `GGUF` (0x47 0x47 0x55 0x46) 로 시작하고,
//! 이어서 리틀엔디언 u32 버전 필드가 옵니다. v1–v3 이 유효한 범위입니다.

use std::io::Read as _;
use std::path::Path;

use lumen_core::{Error, Result};

/// GGUF 파일 헤더에서 추출된 버전 번호.
pub struct GgufHeader {
    /// GGUF 포맷 버전 (1, 2, 또는 3).
    pub version: u32,
}

/// 파일이 유효한 GGUF 헤더를 갖는지 확인합니다.
///
/// 성공 시 헤더 정보를 반환합니다. 실패 시 [`Error::Provenance`].
pub fn sniff(path: &Path) -> Result<GgufHeader> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| Error::Provenance(format!("GGUF 파일 열기 실패 {path:?}: {e}")))?;

    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)
        .map_err(|e| Error::Provenance(format!("GGUF 매직 읽기 실패: {e}")))?;

    if &magic != b"GGUF" {
        return Err(Error::Provenance(format!(
            "GGUF 매직 불일치: expected 47475546, got {:02x}{:02x}{:02x}{:02x}",
            magic[0], magic[1], magic[2], magic[3],
        )));
    }

    let mut ver_bytes = [0u8; 4];
    f.read_exact(&mut ver_bytes)
        .map_err(|e| Error::Provenance(format!("GGUF 버전 읽기 실패: {e}")))?;

    let version = u32::from_le_bytes(ver_bytes);
    if !(1..=3).contains(&version) {
        return Err(Error::Provenance(format!(
            "지원되지 않는 GGUF 버전: {version} (지원: 1–3)"
        )));
    }

    Ok(GgufHeader { version })
}
