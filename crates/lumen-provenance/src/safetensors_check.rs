//! Hugging Face safetensors 파일에 대한 구조 sniff.
//!
//! 의도적으로 전체 파일을 로드하거나 텐서를 인스턴스화하지 않습니다.
//! 목표는 헤더가 파싱되고 내부적으로 일관성 있는지를 확인하는 것 - 그 너머는
//! 추론 엔진의 책임입니다.

use std::path::Path;

use lumen_core::{Error, Result};

/// 파일을 읽어 safetensors 헤더를 파싱합니다.
///
/// 헤더가 선언한 텐서 개수를 반환합니다.
pub fn sniff(path: &Path) -> Result<usize> {
    let bytes = std::fs::read(path)?;
    let view = safetensors::SafeTensors::deserialize(&bytes)
        .map_err(|e| Error::Provenance(format!("safetensors parse: {e}")))?;
    Ok(view.tensors().len())
}
