//! 추론 엔진 바이너리의 매니페스트와 검증.
//!
//! 엔진 실행 파일은 모델 가중치와 같은 공급망 위협 대상입니다. 해시 핀만으로는
//! 핀을 배포하는 정책 파일을 고칠 수 있는 누구나 다른 바이너리를 승인할 수
//! 있으므로, [`EngineManifest`] 는 실행 파일과 그것이 로드하는 공유 라이브러리를
//! 모두 BLAKE3 로 핀하고 그 본문 위에 신뢰된 서명자의 Ed25519 detached 서명을
//! 둡니다. 서명은 파일 바이트가 아니라 매니페스트 본문 (이름, 버전, 해시 집합)
//! 을 덮으므로 검증 비용은 파일당 BLAKE3 한 번과 Ed25519 검증 한 번입니다.
//!
//! # 검증 순서
//! fail-fast 순서로 진행합니다. 서명 (마이크로초) -> 파일 종류와 권한 -> 해시
//! (파일 크기 비례). 신뢰 서명자가 하나라도 설정되면 미서명 매니페스트와 신뢰
//! 집합 밖의 서명자는 거부됩니다. 서명자가 없을 때만 해시 전용으로 통과하며
//! 경고를 남깁니다.

use std::path::{Path, PathBuf};

use lumen_core::{Blake3Hash, Error, Result, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Ed25519 도메인 분리 prefix. 모델 매니페스트 (`lumen.provenance.manifest.v1`)
/// 와 다르므로 같은 키로 서명한 모델 매니페스트를 엔진 매니페스트로 재사용할
/// 수 없습니다.
const ENGINE_SIGN_DOMAIN: &[u8] = b"lumen.provenance.engine.v1";

/// 엔진이 로드하는 부속 파일 (공유 라이브러리 등) 의 핀.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedFile {
    /// 디스크 상의 경로 (매니페스트 디렉토리 기준 상대 또는 절대).
    pub path: PathBuf,
    /// 파일 내용의 BLAKE3 해시.
    pub hash: Blake3Hash,
}

/// 추론 엔진 실행 파일용 매니페스트.
///
/// `path` 와 `files[].path` 는 서명 페이로드에서 제외되고 파일 이름만 포함되므로
/// 배포 위치에 무관하게 서명이 안정적입니다.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineManifest {
    /// 사람이 읽을 수 있는 엔진 이름 (예: `llama-server`).
    pub name: String,
    /// 자유 형식 버전 문자열 (빌드 번호 권장).
    pub version: String,
    /// 실행 파일 경로.
    pub path: PathBuf,
    /// 실행 파일의 BLAKE3 해시.
    pub hash: Blake3Hash,
    /// 실행 파일이 로드하는 부속 파일 (동적 라이브러리 등).
    #[serde(default)]
    pub files: Vec<PinnedFile>,
    /// 옵션 SPDX 라이선스 식별자.
    #[serde(default)]
    pub license: Option<String>,
    /// 정규 서명 페이로드 위에 작성된 detached Ed25519 서명.
    #[serde(default)]
    pub signature: Option<Signature>,
    /// 서명자의 공개 키.
    #[serde(default)]
    pub signer: Option<VerifyingKey>,
}

/// 검증을 통과한 엔진. 경로는 모두 절대 경로로 해석되어 있습니다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedEngine {
    /// 매니페스트 이름.
    pub name: String,
    /// 매니페스트 버전.
    pub version: String,
    /// 실행 파일 경로.
    pub path: PathBuf,
    /// 실행 파일 해시.
    pub hash: Blake3Hash,
    /// 부속 파일 핀 (해석된 경로).
    pub files: Vec<PinnedFile>,
    /// 서명 검증에 사용된 서명자. 해시 전용 통과면 `None`.
    pub signer: Option<VerifyingKey>,
    /// 해시된 총 바이트 수.
    pub size_bytes: u64,
}

impl EngineManifest {
    /// 실행 파일과 부속 파일을 해시해 미서명 매니페스트를 만듭니다.
    ///
    /// # Errors
    /// 파일을 열 수 없거나 일반 파일이 아니면 [`Error::Provenance`].
    pub fn pin(
        name: impl Into<String>,
        version: impl Into<String>,
        path: impl Into<PathBuf>,
        files: &[PathBuf],
    ) -> Result<Self> {
        let path = path.into();
        let hash = hash_regular_file(&path)?.0;
        let files = files
            .iter()
            .map(|p| {
                Ok(PinnedFile {
                    path: p.clone(),
                    hash: hash_regular_file(p)?.0,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            name: name.into(),
            version: version.into(),
            path,
            hash,
            files,
            license: None,
            signature: None,
            signer: None,
        })
    }

    /// 서명이 덮는 바이트.
    ///
    /// `ENGINE_SIGN_DOMAIN || postcard({name | version | hash | [(file_name, hash)] | license})`.
    pub fn signing_payload(&self) -> Result<Vec<u8>> {
        #[derive(Serialize)]
        struct Body<'a> {
            name: &'a str,
            version: &'a str,
            hash: Blake3Hash,
            files: Vec<(String, Blake3Hash)>,
            license: Option<&'a str>,
        }
        let files = self
            .files
            .iter()
            .map(|f| {
                let file_name = f
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .ok_or_else(|| {
                        Error::Provenance(format!("engine file {:?} has no file name", f.path))
                    })?;
                Ok((file_name, f.hash))
            })
            .collect::<Result<Vec<_>>>()?;
        let body = Body {
            name: &self.name,
            version: &self.version,
            hash: self.hash,
            files,
            license: self.license.as_deref(),
        };
        let body_bytes = postcard::to_allocvec(&body)
            .map_err(|e| Error::Decode(format!("engine manifest payload: {e}")))?;
        let mut out = Vec::with_capacity(ENGINE_SIGN_DOMAIN.len() + body_bytes.len());
        out.extend_from_slice(ENGINE_SIGN_DOMAIN);
        out.extend_from_slice(&body_bytes);
        Ok(out)
    }

    /// `key` 로 매니페스트에 서명하고 `signature` 와 `signer` 를 채웁니다.
    pub fn sign_with(&mut self, key: &SigningKey) -> Result<()> {
        let payload = self.signing_payload()?;
        self.signature = Some(key.sign(&payload));
        self.signer = Some(key.verifying_key());
        Ok(())
    }

    /// 서명만 검증합니다 (파일은 건드리지 않음). [`verify_engine`] 의 첫 단계.
    ///
    /// # Errors
    /// - 서명자가 `trusted_signers` 에 없거나 서명이 틀리면 [`Error::Provenance`]
    /// - `trusted_signers` 가 비어있지 않은데 매니페스트가 미서명이면 [`Error::Provenance`]
    pub fn verify_signature(
        &self,
        trusted_signers: &[VerifyingKey],
    ) -> Result<Option<VerifyingKey>> {
        match (&self.signature, &self.signer) {
            (Some(sig), Some(signer)) => {
                if !trusted_signers.iter().any(|t| t == signer) {
                    return Err(Error::Provenance(format!(
                        "engine manifest `{}`: signer {} is not a trusted signer",
                        self.name,
                        signer.to_hex()
                    )));
                }
                let payload = self.signing_payload()?;
                signer.verify(&payload, sig).map_err(|_| {
                    Error::Provenance(format!(
                        "engine manifest `{}`: signature did not verify",
                        self.name
                    ))
                })?;
                Ok(Some(*signer))
            }
            _ if !trusted_signers.is_empty() => Err(Error::Provenance(format!(
                "engine manifest `{}` is unsigned but trusted signers were configured",
                self.name
            ))),
            _ => {
                tracing::warn!(
                    target: "lumen.provenance.engine",
                    engine = %self.name,
                    "엔진 매니페스트가 미서명이며 신뢰 서명자가 없어 해시 전용으로 통과"
                );
                Ok(None)
            }
        }
    }
}

/// 엔진 매니페스트를 끝-끝으로 검증합니다.
///
/// 1. 서명을 `trusted_signers` 로 검증 (파일 접근 전, 가장 저렴).
/// 2. 실행 파일과 모든 부속 파일이 일반 파일이고 world-writable 이 아닌지 확인.
/// 3. 각 파일의 BLAKE3 를 핀과 상수시간 비교.
///
/// # Arguments
/// `base_dir` - 매니페스트의 상대 경로를 해석할 기준 디렉토리
///
/// # Errors
/// 어느 단계라도 실패하면 [`Error::Provenance`]. 실패 시 파일 내용은 사용되지
/// 않습니다.
pub fn verify_engine(
    base_dir: &Path,
    manifest: &EngineManifest,
    trusted_signers: &[VerifyingKey],
) -> Result<VerifiedEngine> {
    let signer = manifest.verify_signature(trusted_signers)?;

    let path = resolve(base_dir, &manifest.path);
    let (actual, mut size_bytes) = hash_regular_file(&path)?;
    if actual != manifest.hash {
        return Err(Error::Provenance(format!(
            "engine `{}`: hash mismatch for {path:?}: manifest={} actual={actual}",
            manifest.name, manifest.hash
        )));
    }

    let mut files = Vec::with_capacity(manifest.files.len());
    for f in &manifest.files {
        let p = resolve(base_dir, &f.path);
        let (h, n) = hash_regular_file(&p)?;
        if h != f.hash {
            return Err(Error::Provenance(format!(
                "engine `{}`: hash mismatch for {p:?}: manifest={} actual={h}",
                manifest.name, f.hash
            )));
        }
        size_bytes += n;
        files.push(PinnedFile { path: p, hash: h });
    }

    tracing::info!(
        target: "lumen.provenance.engine",
        engine = %manifest.name,
        version = %manifest.version,
        hash = %actual,
        files = files.len(),
        signed = signer.is_some(),
        "엔진 검증 완료"
    );
    Ok(VerifiedEngine {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        path,
        hash: actual,
        files,
        signer,
        size_bytes,
    })
}

fn resolve(base_dir: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    }
}

/// 일반 파일인지, world-writable 이 아닌지 확인한 뒤 BLAKE3 와 크기를 반환.
fn hash_regular_file(path: &Path) -> Result<(Blake3Hash, u64)> {
    let meta = std::fs::metadata(path)
        .map_err(|e| Error::Provenance(format!("engine file {path:?}: {e}")))?;
    if !meta.is_file() {
        return Err(Error::Provenance(format!(
            "engine file {path:?} is not a regular file"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o002 != 0 {
            return Err(Error::Provenance(format!(
                "engine file {path:?} is world-writable; refusing to trust it"
            )));
        }
    }
    let hash = Blake3Hash::of_file(path)
        .map_err(|e| Error::Provenance(format!("engine file {path:?}: hash: {e}")))?;
    Ok((hash, meta.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::OsRng;

    fn fixture() -> (tempfile::TempDir, EngineManifest) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("engine");
        let lib = dir.path().join("libengine.so");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(&lib, b"library bytes").unwrap();
        let mut m = EngineManifest::pin("engine", "1", &bin, std::slice::from_ref(&lib)).unwrap();
        m.path = PathBuf::from("engine");
        m.files[0].path = PathBuf::from("libengine.so");
        (dir, m)
    }

    #[test]
    fn pin_hashes_relative_to_cwd_only_when_relative() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("engine");
        std::fs::write(&bin, b"x").unwrap();
        let m = EngineManifest::pin("e", "1", &bin, &[]).unwrap();
        assert_eq!(m.hash, Blake3Hash::of(b"x"));
        assert!(m.signature.is_none());
    }

    #[test]
    fn unsigned_passes_only_without_trusted_signers() {
        let (dir, mut m) = fixture();
        m.hash = Blake3Hash::of(b"#!/bin/sh\nexit 0\n");
        let v = verify_engine(dir.path(), &m, &[]).unwrap();
        assert!(v.signer.is_none());
        assert_eq!(v.files.len(), 1);
        assert!(v.path.is_absolute());

        let sk = SigningKey::generate(&mut OsRng);
        let err = verify_engine(dir.path(), &m, &[sk.verifying_key()]).unwrap_err();
        assert!(err.to_string().contains("unsigned"), "{err}");
    }

    #[test]
    fn signed_by_trusted_signer_passes_and_binds_all_files() {
        let (dir, mut m) = fixture();
        let sk = SigningKey::generate(&mut OsRng);
        m.sign_with(&sk).unwrap();
        let v = verify_engine(dir.path(), &m, &[sk.verifying_key()]).unwrap();
        assert_eq!(v.signer, Some(sk.verifying_key()));

        // 부속 파일 변조 -> 거부.
        std::fs::write(dir.path().join("libengine.so"), b"evil").unwrap();
        let err = verify_engine(dir.path(), &m, &[sk.verifying_key()]).unwrap_err();
        assert!(err.to_string().contains("hash mismatch"), "{err}");
    }

    #[test]
    fn untrusted_signer_and_tampered_body_rejected() {
        let (dir, mut m) = fixture();
        let sk = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
        m.sign_with(&sk).unwrap();

        let err = verify_engine(dir.path(), &m, &[other.verifying_key()]).unwrap_err();
        assert!(err.to_string().contains("not a trusted signer"), "{err}");

        // 서명 후 본문 (버전) 변경 -> 서명 불일치. 파일은 열리지 않아야 함.
        let mut tampered = m.clone();
        tampered.version = "2".into();
        let err =
            verify_engine(Path::new("/nonexistent"), &tampered, &[sk.verifying_key()]).unwrap_err();
        assert!(err.to_string().contains("did not verify"), "{err}");

        // 부속 파일 이름 변경도 서명에 묶임.
        let mut renamed = m.clone();
        renamed.files[0].path = PathBuf::from("libother.so");
        let err =
            verify_engine(Path::new("/nonexistent"), &renamed, &[sk.verifying_key()]).unwrap_err();
        assert!(err.to_string().contains("did not verify"), "{err}");
    }

    #[test]
    fn engine_domain_differs_from_model_manifest_domain() {
        let (_dir, m) = fixture();
        let payload = m.signing_payload().unwrap();
        assert!(payload.starts_with(ENGINE_SIGN_DOMAIN));
        assert!(!payload.starts_with(b"lumen.provenance.manifest.v1"));
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_engine_refused() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, mut m) = fixture();
        let bin = dir.path().join("engine");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o757)).unwrap();
        let err = verify_engine(dir.path(), &m, &[]).unwrap_err();
        assert!(err.to_string().contains("world-writable"), "{err}");
        m.files.clear();
        assert!(verify_engine(dir.path(), &m, &[]).is_err());
    }

    #[test]
    fn toml_round_trip_keeps_signature() {
        let (_dir, mut m) = fixture();
        let sk = SigningKey::generate(&mut OsRng);
        m.sign_with(&sk).unwrap();
        let text = toml::to_string(&m).unwrap();
        let back: EngineManifest = toml::from_str(&text).unwrap();
        assert_eq!(back.signature, m.signature);
        assert_eq!(back.signer, m.signer);
        assert_eq!(back.files, m.files);
        back.verify_signature(&[sk.verifying_key()]).unwrap();
    }
}
