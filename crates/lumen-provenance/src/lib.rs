//! 모델 파일 provenance: 해시 + 서명 검증 + SBOM 발행.
//!
//! 제로 트러스트 배포에서 우리는 런타임의 파일시스템 내용을 신뢰할 수
//! 없습니다. Lumen 은 모든 모델 산출물이 BLAKE3 해시와 (선택적으로) 신뢰된
//! 서명자의 Ed25519 서명을 핀하는 [`ModelManifest`] 로 기술되도록 요구합니다.
//! 모든 로드는 [`verify_model`] 을 거치며 - 어떤 불일치라도 바이트가 추론
//! 엔진에 도달하기 전에 로드를 중단시킵니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod gguf;
pub mod manifest;
pub mod onnx;
pub mod pinset;
pub mod safetensors_check;
pub mod sbom;

use std::path::Path;

use lumen_core::{Blake3Hash, Error, Result, VerifyingKey};

pub use manifest::{Format, ModelManifest, SignedManifest};
pub use pinset::{verify_model_against_pinset, PinAcceptance, PinEntry, PinSet, RetiredPin};
pub use sbom::{generate_sbom, SbomComponent, SbomDocument};

/// 검증된 모델에 대해 반환되는 정보.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    /// 사용된 매니페스트의 이름.
    pub name: String,
    /// 매니페스트 버전 문자열.
    pub version: String,
    /// 감지된 형식.
    pub format: Format,
    /// 매니페스트와 일치한 해시.
    pub hash: Blake3Hash,
    /// 해시된 바이트 수.
    pub size_bytes: u64,
}

/// 모델 파일을 매니페스트와 끝-끝으로 검증합니다.
///
/// 단계:
///
/// 1. 파일 위에서 BLAKE3 계산.
/// 2. `manifest.hash` 와 비교. 불일치 ⇒ 중단.
/// 3. 서명이 있으면 `trusted_signers` 중 하나로 검증.
/// 4. 형식별 구조 sniff (Safetensors 헤더 파싱 등).
pub fn verify_model(
    path: &Path,
    manifest: &ModelManifest,
    trusted_signers: &[VerifyingKey],
) -> Result<ModelInfo> {
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();

    let actual = Blake3Hash::of_file(path)?;
    if actual != manifest.hash {
        return Err(Error::Provenance(format!(
            "hash mismatch for {}: manifest={} actual={}",
            manifest.name, manifest.hash, actual
        )));
    }

    // trusted_signers 가 비어있지 않다면 매니페스트는 반드시 서명되어
    // 있어야 합니다 - 이전 구현은 None 매니페스트를 silently skip 하여
    // 미서명 모델이 hash 매칭만으로 통과될 수 있는 결함이 있었습니다.
    match (&manifest.signature, &manifest.signer) {
        (Some(sig), Some(signer)) => {
            let body = manifest.signing_payload()?;
            let mut accepted = false;
            for trusted in trusted_signers {
                if trusted == signer && trusted.verify(&body, sig).is_ok() {
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                return Err(Error::Provenance(
                    "signature did not verify against any trusted signer".into(),
                ));
            }
        }
        _ if !trusted_signers.is_empty() => {
            return Err(Error::Provenance(
                "manifest is unsigned but trusted signers were configured".into(),
            ));
        }
        _ => {}
    }

    match manifest.format {
        Format::Safetensors => {
            let _tensor_count = safetensors_check::sniff(path)?;
        }
        Format::Onnx => {
            let _onnx_header = onnx::sniff(path)?;
        }
        Format::Gguf => {
            let _gguf_header = gguf::sniff(path)?;
        }
    }

    Ok(ModelInfo {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        format: manifest.format,
        hash: actual,
        size_bytes,
    })
}
