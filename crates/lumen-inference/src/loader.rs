//! 검증 강제형 모델 로더.
//!
//! 제로 트러스트 배포에서 모델 파일은 신뢰할 수 없습니다. 이 모듈은
//! [`VerifiedModelLoader`] 를 통해 모든 로드 요청에 BLAKE3 해시 + 선택적
//! Ed25519 서명 검증을 강제합니다.
//!
//! [`VerifiedModelHandle`] 은 공개 생성자가 없으므로, 검증을 우회한 핸들
//! 생성은 컴파일 단계에서 불가능합니다.

use std::path::{Path, PathBuf};

use lumen_core::{Blake3Hash, Error, Result, VerifyingKey};
use lumen_provenance::{verify_model, Format, ModelInfo, ModelManifest};

/// 검증을 통과한 모델 파일에 대한 불투명 핸들.
///
/// [`VerifiedModelLoader`] 만이 이 타입의 값을 생성할 수 있습니다.
/// 백엔드 엔진은 raw 경로 대신 이 핸들을 받아 보안 불변식을 보장합니다.
#[derive(Debug, Clone)]
pub struct VerifiedModelHandle {
    /// 검증된 파일의 정규화된 절대 경로.
    pub(crate) path: PathBuf,
    /// `verify_model` 이 반환한 메타데이터 (이름, 버전, 형식, 해시).
    pub model_info: ModelInfo,
}

impl VerifiedModelHandle {
    /// 검증된 모델 파일의 경로.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 신뢰 서명자 목록을 보유하고 모든 모델 로드를 중개합니다.
///
/// ## 사용 예
///
/// ```rust,ignore
/// use lumen_inference::loader::VerifiedModelLoader;
/// use lumen_provenance::{Format, ModelManifest};
///
/// let loader = VerifiedModelLoader::hash_only();
/// let handle = loader.load(&path, &manifest)?;
/// let engine = CandleLlmEngine::from_gguf(&handle, tokenizer_path)?;
/// ```
pub struct VerifiedModelLoader {
    trusted_signers: Vec<VerifyingKey>,
}

impl VerifiedModelLoader {
    /// 주어진 신뢰 서명자 목록으로 로더를 생성합니다.
    ///
    /// 빈 슬라이스를 전달하면 서명 없이 BLAKE3 해시만 확인합니다.
    pub fn new(trusted_signers: Vec<VerifyingKey>) -> Self {
        Self { trusted_signers }
    }

    /// 서명 검증 없이 BLAKE3 해시만 확인하는 로더.
    pub fn hash_only() -> Self {
        Self {
            trusted_signers: Vec::new(),
        }
    }

    /// `path` 의 파일을 `manifest` 와 대조해 검증하고 핸들을 반환합니다.
    ///
    /// - BLAKE3 해시 불일치 → [`Error::Provenance`]
    /// - 서명 검증 실패 → [`Error::Provenance`]
    /// - 형식별 헤더 구조 오류 → [`Error::Provenance`]
    pub fn load(&self, path: &Path, manifest: &ModelManifest) -> Result<VerifiedModelHandle> {
        let model_info = verify_model(path, manifest, &self.trusted_signers)?;
        let canonical = path
            .canonicalize()
            .map_err(|e| Error::Inference(format!("경로 정규화 실패 {path:?}: {e}")))?;
        tracing::info!(
            target: "lumen.inference.loader",
            name = %model_info.name,
            version = %model_info.version,
            hash = %model_info.hash,
            bytes = model_info.size_bytes,
            "모델 검증 완료"
        );
        Ok(VerifiedModelHandle {
            path: canonical,
            model_info,
        })
    }

    /// 매니페스트 없이 BLAKE3 해시를 온라인으로 계산해 검증합니다.
    ///
    /// `expected_hash` 와 일치하지 않으면 에러. 서명은 확인하지 않습니다.
    pub fn load_hash_only(
        &self,
        path: &Path,
        expected_hash: Blake3Hash,
        name: impl Into<String>,
        version: impl Into<String>,
        format: Format,
    ) -> Result<VerifiedModelHandle> {
        let manifest = ModelManifest {
            name: name.into(),
            version: version.into(),
            path: path.to_owned(),
            format,
            hash: expected_hash,
            license: None,
            signature: None,
            signer: None,
        };
        self.load(path, &manifest)
    }
}
