//! `llama-server` 자식 프로세스 감독.
//!
//! 엔진 바이너리는 모델 파일과 같은 수준의 공급망 위협 대상입니다. 따라서
//! 기동 전에 BLAKE3 해시를 핀과 대조하고, `LLAMA_ARG_*` 환경변수를 모두
//! 제거해 외부에서 인자를 주입할 수 없게 하며, API 키는 argv 가 아닌
//! 환경변수로만 전달합니다 (`ps` 노출 방지). 프로세스는 [`LlamaServerProcess`]
//! 가 drop 될 때 함께 종료됩니다.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use lumen_core::{Blake3Hash, Error, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use zeroize::Zeroizing;

use lumen_provenance::{PinnedFile, VerifiedEngine};

use crate::loader::VerifiedModelHandle;

use super::Endpoint;

/// `llama-server` 기동 명세.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    /// `llama-server` 바이너리 경로.
    pub binary: PathBuf,
    /// 바이너리의 BLAKE3 핀. 불일치 시 기동 거부.
    pub binary_hash: Blake3Hash,
    /// 바이너리가 로드하는 부속 파일 (공유 라이브러리 등) 의 핀. 기동 직전
    /// 바이너리와 함께 재검증합니다.
    pub pinned_files: Vec<PinnedFile>,
    /// 검증된 GGUF 모델 핸들.
    pub model: VerifiedModelHandle,
    /// 서버가 바인드할 엔드포인트. UDS 권장.
    pub endpoint: Endpoint,
    /// KV 캐시 컨텍스트 길이 (`-c`).
    pub n_ctx: u32,
    /// CPU 스레드 수 (`-t`). `None` 이면 서버 기본값.
    pub n_threads: Option<u32>,
    /// GPU 오프로드 레이어 수 (`-ngl`). `None` 이면 서버 기본값.
    pub n_gpu_layers: Option<u32>,
    /// 병렬 슬롯 수 (`--parallel`). 결정론이 필요하면 1.
    pub parallel: u32,
    /// 그대로 전달할 추가 인자. `--api-key`, `-m`, `--host`, `--port` 는 금지.
    pub extra_args: Vec<String>,
    /// `/health` 가 `ok` 가 될 때까지 기다리는 최대 시간.
    pub startup_timeout: Duration,
}

impl SpawnSpec {
    /// 필수 값만으로 생성. 나머지는 보수적 기본값.
    pub fn new(
        binary: impl Into<PathBuf>,
        binary_hash: Blake3Hash,
        model: VerifiedModelHandle,
        endpoint: Endpoint,
    ) -> Self {
        Self {
            binary: binary.into(),
            binary_hash,
            pinned_files: Vec::new(),
            model,
            endpoint,
            n_ctx: 4096,
            n_threads: None,
            n_gpu_layers: None,
            parallel: 1,
            extra_args: Vec::new(),
            startup_timeout: Duration::from_secs(120),
        }
    }

    /// [`lumen_provenance::verify_engine`] 을 통과한 엔진으로부터 생성합니다.
    /// 실행 파일과 부속 파일 핀이 모두 기동 시 재검증 대상이 됩니다.
    pub fn from_engine(
        engine: &VerifiedEngine,
        model: VerifiedModelHandle,
        endpoint: Endpoint,
    ) -> Self {
        let mut spec = Self::new(engine.path.clone(), engine.hash, model, endpoint);
        spec.pinned_files = engine.files.clone();
        spec
    }
}

/// 감독 중인 자식 프로세스.
pub struct LlamaServerProcess {
    child: Child,
    socket_path: Option<PathBuf>,
}

impl std::fmt::Debug for LlamaServerProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaServerProcess")
            .field("pid", &self.child.id())
            .field("socket_path", &self.socket_path)
            .finish()
    }
}

/// argv 로 넘기면 안 되는 옵션 (Lumen 이 직접 제어).
const RESERVED_ARGS: &[&str] = &[
    "--api-key",
    "--api-key-file",
    "-m",
    "--model",
    "--host",
    "--port",
    "-hf",
    "--hf-repo",
    "--hf-file",
    "-mu",
    "--model-url",
];

impl LlamaServerProcess {
    /// 바이너리 해시를 검증한 뒤 서버를 기동합니다. 준비 완료 대기는 하지
    /// 않습니다 (엔진이 `/health` 로 확인).
    ///
    /// # Errors
    /// - 바이너리 해시 불일치 / 파일 아님 -> [`Error::Provenance`]
    /// - 예약 인자 사용 -> [`Error::Invalid`]
    /// - spawn 실패 -> [`Error::Io`]
    pub fn spawn(spec: &SpawnSpec, api_key: &Zeroizing<String>) -> Result<Self> {
        verify_binary(&spec.binary, &spec.binary_hash)?;
        for f in &spec.pinned_files {
            verify_binary(&f.path, &f.hash)?;
        }
        for a in &spec.extra_args {
            let name = a.split('=').next().unwrap_or(a);
            if RESERVED_ARGS.contains(&name) {
                return Err(Error::Invalid(format!(
                    "llama-server: extra arg `{name}` is reserved and controlled by lumen"
                )));
            }
        }

        let mut cmd = Command::new(&spec.binary);
        cmd.arg("-m").arg(spec.model.path());
        cmd.arg("-c").arg(spec.n_ctx.to_string());
        cmd.arg("--parallel").arg(spec.parallel.to_string());
        cmd.arg("--no-webui");
        if let Some(t) = spec.n_threads {
            cmd.arg("-t").arg(t.to_string());
        }
        if let Some(g) = spec.n_gpu_layers {
            cmd.arg("-ngl").arg(g.to_string());
        }
        let mut socket_path = None;
        match &spec.endpoint {
            Endpoint::Unix(path) => {
                if path.extension().and_then(|e| e.to_str()) != Some("sock") {
                    return Err(Error::Invalid(
                        "llama-server: unix socket path must end with `.sock`".into(),
                    ));
                }
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
                cmd.arg("--host").arg(path);
                socket_path = Some(path.clone());
            }
            Endpoint::Tcp(addr) => {
                cmd.arg("--host").arg(addr.ip().to_string());
                cmd.arg("--port").arg(addr.port().to_string());
            }
        }
        cmd.args(&spec.extra_args);

        // 외부 환경에서 숨은 인자를 주입하지 못하도록 LLAMA_ARG_* 를 전부
        // 제거한 뒤 API 키만 다시 설정합니다.
        for (k, _) in std::env::vars_os() {
            let name = k.to_string_lossy();
            if name.starts_with("LLAMA_ARG_") || name == "LLAMA_API_KEY" {
                cmd.env_remove(&k);
            }
        }
        cmd.env("LLAMA_API_KEY", api_key.as_str());
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let pid = child.id();
        tracing::info!(
            target: "lumen.inference.llama.process",
            pid,
            binary = %spec.binary.display(),
            model = %spec.model.model_info.name,
            endpoint = %spec.endpoint,
            "llama-server 기동"
        );
        if let Some(out) = child.stdout.take() {
            tokio::spawn(drain_lines(out, "stdout"));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(drain_lines(err, "stderr"));
        }
        Ok(Self { child, socket_path })
    }

    /// OS 프로세스 ID.
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// 프로세스가 이미 종료되었으면 종료 상태를 반환합니다.
    pub fn try_exit_status(&mut self) -> Result<Option<std::process::ExitStatus>> {
        Ok(self.child.try_wait()?)
    }

    /// 프로세스를 종료하고 소켓 파일을 정리합니다.
    pub async fn shutdown(mut self) -> Result<()> {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        self.cleanup_socket();
        Ok(())
    }

    fn cleanup_socket(&mut self) {
        if let Some(p) = self.socket_path.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl Drop for LlamaServerProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.cleanup_socket();
    }
}

/// 바이너리 (또는 부속 파일) 가 일반 파일이고 BLAKE3 가 핀과 일치하는지 확인합니다.
pub fn verify_binary(path: &Path, expected: &Blake3Hash) -> Result<()> {
    let meta = std::fs::metadata(path)
        .map_err(|e| Error::Provenance(format!("engine binary {path:?}: {e}")))?;
    if !meta.is_file() {
        return Err(Error::Provenance(format!(
            "engine binary {path:?} is not a regular file"
        )));
    }
    let actual = Blake3Hash::of_file(path)
        .map_err(|e| Error::Provenance(format!("engine binary {path:?}: hash: {e}")))?;
    if actual != *expected {
        return Err(Error::Provenance(format!(
            "engine binary hash mismatch for {path:?}: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

async fn drain_lines<R: tokio::io::AsyncRead + Unpin>(reader: R, stream: &'static str) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(target: "lumen.inference.llama.process", stream, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_hash_mismatch_rejected() {
        let dir = std::env::temp_dir().join(format!("lumen-llama-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("fake-llama-server");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
        let wrong = Blake3Hash::of(b"not the file");
        let err = verify_binary(&bin, &wrong).unwrap_err();
        assert!(matches!(err, Error::Provenance(_)), "{err}");
        let right = Blake3Hash::of_file(&bin).unwrap();
        verify_binary(&bin, &right).unwrap();
        assert!(verify_binary(&dir, &right).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn pinned_file_mismatch_blocks_spawn() {
        let dir = std::env::temp_dir().join(format!("lumen-llama-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("fake-llama-server");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();
        let lib = dir.join("libfake.so");
        std::fs::write(&lib, b"lib").unwrap();
        let model_path = dir.join("m.gguf");
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 24]);
        std::fs::write(&model_path, &bytes).unwrap();
        let model = crate::loader::VerifiedModelLoader::hash_only()
            .load_hash_only(
                &model_path,
                Blake3Hash::of(&bytes),
                "m",
                "0",
                lumen_provenance::Format::Gguf,
            )
            .unwrap();
        let mut spec = SpawnSpec::new(
            &bin,
            Blake3Hash::of_file(&bin).unwrap(),
            model,
            Endpoint::Unix(dir.join("s.sock")),
        );
        spec.pinned_files = vec![PinnedFile {
            path: lib.clone(),
            hash: Blake3Hash::of(b"tampered"),
        }];
        let key = Zeroizing::new("k".to_owned());
        let err = LlamaServerProcess::spawn(&spec, &key).unwrap_err();
        assert!(matches!(err, Error::Provenance(_)), "{err}");
        assert!(err.to_string().contains("libfake.so"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
