//! 디스크 상의 정책 파일 형식.
//!
//! 정책 번들은 다음을 운반하는 단일 TOML 문서입니다:
//! - 에이전트의 신원 (`AgentId`),
//! - 신뢰된 issuer 공개 키,
//! - 에이전트가 행사할 수 있는 capability 집합,
//! - 모델 / 엔진 매니페스트 서명을 검증할 신뢰 서명자 공개 키,
//! - 에이전트 실행 전 검증되어야 하는 모델 매니페스트와 엔진 매니페스트.
//!
//! CLI 의 `run` 서브커맨드는 파일의 BLAKE3 가 명령행으로 제공된 핀과 일치
//! 해야 한다고 요구합니다 - 디스크 내용에 대한 묵시적 신뢰는 없습니다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lumen_capability::capability::Capability;
use lumen_core::{AgentId, Blake3Hash, Result, VerifyingKey};
use lumen_provenance::manifest::ModelManifest;
use lumen_provenance::EngineManifest;
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
    /// 모델 / 엔진 매니페스트 서명을 검증할 신뢰 서명자 (Ed25519, hex).
    /// 하나라도 있으면 미서명 매니페스트는 거부됩니다.
    #[serde(default)]
    pub trusted_signers: Vec<VerifyingKey>,
    /// 옵션 모델 매니페스트.
    #[serde(default)]
    pub models: Vec<ModelManifest>,
    /// 옵션 엔진 매니페스트. `[inference.params].binary` 가 이름으로 참조합니다.
    #[serde(default)]
    pub engines: Vec<EngineManifest>,
    /// 추적성을 위해 정책 해시에 포함되는 옵션 스냅샷 라벨.
    #[serde(default)]
    pub snapshot: Option<String>,
    /// 추론 백엔드 선택. 생략하면 `dummy`.
    #[serde(default)]
    pub inference: InferenceSection,
}

/// inference 섹션.
///
/// `backend` 는 [`lumen_inference::BackendRegistry`] 에 등록된 이름이고,
/// `params` 는 그 백엔드가 이해하는 키/값입니다. 정책 파일 전체가 BLAKE3
/// 로 핀되므로 엔진 바이너리 해시 (`binary_hash`) 와 모델 해시도 함께
/// 핀됩니다. `model` 이 `[[models]]` 의 이름과 같으면 그 매니페스트의 경로 /
/// 해시 / 이름 / 버전으로 치환됩니다.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceSection {
    /// 백엔드 이름 (`dummy`, `llama-server`, ...).
    #[serde(default = "default_backend")]
    pub backend: String,
    /// 백엔드별 파라미터.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

fn default_backend() -> String {
    "dummy".into()
}

impl Default for InferenceSection {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            params: BTreeMap::new(),
        }
    }
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
        Self::resolve_path(policy_dir, &manifest.path)
    }

    fn resolve_path(policy_dir: &Path, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            policy_dir.join(path)
        }
    }

    /// 백엔드 파라미터를 반환하되, `model` 이 `[[models]]` 의 이름을 가리키면
    /// 매니페스트의 경로 / 해시 / 이름 / 버전으로, `binary` 가 `[[engines]]` 의
    /// 이름을 가리키면 실행 파일 경로 / 해시 / 부속 파일 핀 (`binary_files`,
    /// JSON) 으로 치환합니다. 상대 경로 파라미터 (`model`, `binary`,
    /// `api_key_file`) 는 `policy_dir` 기준으로 절대화합니다.
    ///
    /// 엔진 매니페스트의 서명 검증은 여기서 하지 않습니다. 호출자가
    /// [`lumen_provenance::verify_engine`] 을 먼저 통과시켜야 합니다.
    pub fn inference_params(&self, policy_dir: &Path) -> BTreeMap<String, String> {
        let mut params = self.inference.params.clone();
        if let Some(binary) = params.get("binary").cloned() {
            if let Some(e) = self.engines.iter().find(|e| e.name == binary) {
                let path = Self::resolve_path(policy_dir, &e.path);
                params.insert("binary".into(), path.to_string_lossy().into_owned());
                params.insert("binary_hash".into(), e.hash.to_hex());
                let files: Vec<lumen_provenance::PinnedFile> = e
                    .files
                    .iter()
                    .map(|f| lumen_provenance::PinnedFile {
                        path: Self::resolve_path(policy_dir, &f.path),
                        hash: f.hash,
                    })
                    .collect();
                if !files.is_empty() {
                    params.insert(
                        "binary_files".into(),
                        serde_json::to_string(&files).unwrap_or_else(|_| "[]".into()),
                    );
                }
            }
        }
        if let Some(model) = params.get("model").cloned() {
            if let Some(m) = self.models.iter().find(|m| m.name == model) {
                let path = Self::resolve_model_path(policy_dir, m);
                params.insert("model".into(), path.to_string_lossy().into_owned());
                params.insert("model_hash".into(), m.hash.to_hex());
                params.insert("model_name".into(), m.name.clone());
                params.insert("model_version".into(), m.version.clone());
            }
        }
        for key in ["model", "binary", "api_key_file"] {
            if let Some(v) = params.get(key) {
                let p = Path::new(v);
                if !p.is_absolute() {
                    params.insert(
                        key.into(),
                        policy_dir.join(p).to_string_lossy().into_owned(),
                    );
                }
            }
        }
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_reference_is_substituted_with_pins() {
        let policy: PolicyFile = toml::from_str(
            r#"
[agent]
id = "0102030405060708090a0b0c0d0e0f10"

[[engines]]
name = "llama-server"
version = "b1"
path = "bin/llama-server"
hash = "1111111111111111111111111111111111111111111111111111111111111111"
files = [{ path = "lib/libllama.so", hash = "2222222222222222222222222222222222222222222222222222222222222222" }]

[inference]
backend = "llama-server"

[inference.params]
mode = "spawn"
endpoint = "unix:/run/l.sock"
binary = "llama-server"
"#,
        )
        .unwrap();
        let params = policy.inference_params(Path::new("/etc/lumen"));
        assert_eq!(params["binary"], "/etc/lumen/bin/llama-server");
        assert_eq!(params["binary_hash"], "1".repeat(64));
        let files: Vec<lumen_provenance::PinnedFile> =
            serde_json::from_str(&params["binary_files"]).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/etc/lumen/lib/libllama.so"));
        assert_eq!(files[0].hash.to_hex(), "2".repeat(64));
        assert!(policy.trusted_signers.is_empty());
    }
}
