//! 단일 모델 산출물의 매니페스트 스키마 (TOML/JSON).

use std::path::PathBuf;

use lumen_core::{Blake3Hash, Error, Result, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

/// 매니페스트가 기술하는 파일 형식.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Hugging Face safetensors.
    Safetensors,
    /// ONNX (구조 검증은 v0.3 부터 활성화).
    Onnx,
    /// GGUF (llama.cpp / candle-transformers 양자화 가중치).
    Gguf,
}

/// 단일 모델 산출물용 매니페스트.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelManifest {
    /// 사람이 읽을 수 있는 모델 이름.
    pub name: String,
    /// 자유 형식 버전 문자열 (semver 권장).
    pub version: String,
    /// 디스크 상의 경로 (매니페스트 디렉토리 기준 상대 또는 절대).
    pub path: PathBuf,
    /// 감지된 형식.
    pub format: Format,
    /// 파일 내용의 BLAKE3 해시.
    pub hash: Blake3Hash,
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

impl ModelManifest {
    /// 서명이 덮는 바이트.
    ///
    /// `name | version | format | hash | license` 를 연접합니다. 경로를
    /// 제외하면 배포 위치에 무관하게 서명이 안정적입니다.
    pub fn signing_payload(&self) -> Result<Vec<u8>> {
        #[derive(Serialize)]
        struct Body<'a> {
            name: &'a str,
            version: &'a str,
            format: Format,
            hash: Blake3Hash,
            license: Option<&'a str>,
        }
        let body = Body {
            name: &self.name,
            version: &self.version,
            format: self.format,
            hash: self.hash,
            license: self.license.as_deref(),
        };
        postcard::to_allocvec(&body).map_err(|e| Error::Decode(format!("manifest payload: {e}")))
    }

    /// `key` 로 매니페스트에 서명하고 `signature` 와 `signer` 를 채웁니다.
    pub fn sign_with(&mut self, key: &SigningKey) -> Result<()> {
        let payload = self.signing_payload()?;
        self.signature = Some(key.sign(&payload));
        self.signer = Some(key.verifying_key());
        Ok(())
    }
}

/// 정책 파일에 여러 매니페스트를 담을 때 사용되는 작은 wrapper.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SignedManifest {
    /// 모델 항목들.
    pub models: Vec<ModelManifest>,
}
