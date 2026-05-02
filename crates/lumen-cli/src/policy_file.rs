//! 디스크 상의 정책 파일 형식.
//!
//! 정책 번들은 다음을 운반하는 단일 TOML 문서입니다:
//! - 에이전트의 신원 (`AgentId`),
//! - 신뢰된 issuer 공개 키,
//! - 에이전트가 행사할 수 있는 capability 집합,
//! - 에이전트 실행 전 검증되어야 하는 모델 매니페스트.
//!
//! CLI 의 `run` 서브커맨드는 파일의 BLAKE3 가 명령행으로 제공된 핀과 일치
//! 해야 한다고 요구합니다 - 디스크 내용에 대한 묵시적 신뢰는 없습니다.

use std::path::{Path, PathBuf};

use lumen_capability::capability::Capability;
use lumen_core::{AgentId, Blake3Hash, Result, VerifyingKey};
use lumen_provenance::manifest::ModelManifest;
use serde::{Deserialize, Serialize};

/// 최상위 정책 번들.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyFile {
    /// 에이전트 신원.
    pub agent: AgentSection,
    /// 신뢰된 issuer 공개 키 (Ed25519, hex 인코딩).
    #[serde(default)]
    pub trusted_issuers: Vec<VerifyingKey>,
    /// Capability 토큰들.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// 옵션 모델 매니페스트.
    #[serde(default)]
    pub models: Vec<ModelManifest>,
    /// 추적성을 위해 정책 해시에 포함되는 옵션 스냅샷 라벨.
    #[serde(default)]
    pub snapshot: Option<String>,
}

/// agent 섹션의 키.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSection {
    /// 에이전트 ID.
    pub id: AgentId,
}

impl PolicyFile {
    /// 정책 파일을 디스크에서 읽고 디코딩합니다.
    ///
    /// 호출자가 핀된 해시와 비교한 뒤 문서를 신뢰할 수 있도록 같은 호출에서
    /// 파일의 BLAKE3 를 함께 계산합니다.
    pub fn load(path: &Path) -> Result<(Self, Blake3Hash)> {
        let bytes = std::fs::read(path)?;
        let hash = Blake3Hash::of(&bytes);
        let parsed: Self = toml::from_str(
            std::str::from_utf8(&bytes)
                .map_err(|e| lumen_core::Error::Decode(format!("policy not utf-8: {e}")))?,
        )
        .map_err(|e| lumen_core::Error::Decode(format!("policy parse: {e}")))?;
        Ok((parsed, hash))
    }

    /// `path` 가 상대 경로면 `policy_dir` 기준으로 해석합니다.
    pub fn resolve_model_path(policy_dir: &Path, manifest: &ModelManifest) -> PathBuf {
        if manifest.path.is_absolute() {
            manifest.path.clone()
        } else {
            policy_dir.join(&manifest.path)
        }
    }
}
