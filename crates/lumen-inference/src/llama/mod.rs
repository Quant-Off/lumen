//! llama.cpp 백엔드: 격리된 `llama-server` 프로세스 연동.
//!
//! # 왜 in-process FFI 가 아닌 프로세스 격리인가
//!
//! - **감사 표면**: llama.cpp 는 수십만 줄의 C/C++ 입니다. FFI 로 링크하면
//!   `#![forbid(unsafe_code)]` 가 무의미해지고, 메모리 안전성 위반이 Lumen
//!   호스트 (정책 엔진, capability 검증, 키 자료) 와 같은 주소 공간에서
//!   일어납니다. 별도 프로세스면 크래시 / 취약점이 소켓 경계에서 멈춥니다.
//! - **폐쇄망 빌드**: `llama-cpp-2` 는 cmake + bindgen + libclang + (선택)
//!   CUDA 툴체인을 Rust 빌드에 끌어옵니다. 프로세스 격리 방식은 Rust 쪽이
//!   순수 Rust 로 유지되고, 운영자는 어차피 GPU 용으로 따로 빌드해야 하는
//!   `llama-server` 바이너리를 BLAKE3 핀과 함께 반입하면 됩니다.
//! - **TEE 배치 유연성**: 서버는 호스트 TEE 안에 두고, 샌드박스는
//!   [`crate::ChannelEngine`] 로 그 앞의 Lumen 호스트에만 도달합니다. 같은
//!   엔진 코드가 두 배치 모두에 쓰입니다.
//!
//! # 신뢰 경계
//!
//! | 항목            | 검증 방법                                         |
//! |-----------------|---------------------------------------------------|
//! | 엔진 바이너리    | BLAKE3 핀 (`SpawnSpec::binary_hash`), 기동 전 검사   |
//! | 모델 파일        | [`crate::VerifiedModelLoader`] (BLAKE3 + Ed25519)    |
//! | 서버 -> 모델 경로 | `/props.model_path` 가 검증된 핸들 경로와 일치해야 함 |
//! | 요청 인증        | 프로세스마다 새로 생성한 256-bit API 키 (env 전달)   |
//! | 전송             | UDS 또는 loopback TCP 만 허용. 원격은 SecureChannel  |
//! | 응답 크기        | [`crate::http`] 의 헤더 / 바디 / SSE 상한             |
//! | 인자 주입        | `LLAMA_ARG_*` 환경변수 제거, 예약 인자 거부           |
//!
//! # 결정론
//!
//! `temperature = 0`, 고정 `seed`, `--parallel 1` 조합에서 동일 바이너리 /
//! 하드웨어 / 모델에 대해 출력이 재현됩니다. 슬롯이 여러 개면 배치 구성에
//! 따라 부동소수 누적 순서가 달라질 수 있어 [`crate::EngineCapabilities::seed_deterministic`]
//! 은 `total_slots == 1` 일 때만 `true` 입니다.

pub mod process;
pub mod protocol;

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream;
use lumen_core::{Blake3Hash, Error, Result, Rng};
use serde::de::DeserializeOwned;
use serde::Serialize;
use zeroize::Zeroizing;

use crate::http::{self, IoStream, Request, Response, SseReader};
use crate::loader::{VerifiedModelHandle, VerifiedModelLoader};
use crate::streaming::{StreamingEngine, Token, TokenStream};
use crate::toolcall::parse_tool_call;
use crate::{Completion, EngineCapabilities, EngineInfo, InferenceEngine, SamplingParams};

pub use process::{LlamaServerProcess, SpawnSpec};
use protocol::{CompletionChunk, CompletionRequest, ErrorBody, Health, Props};

/// `/health` 폴링 간격.
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// 서버 접속 엔드포인트.
///
/// 평문 HTTP 이므로 로컬 전송만 허용합니다. TCP 는 loopback 주소여야 하며
/// 그 외 주소는 [`Endpoint::parse`] 와 접속 시점 모두에서 거부됩니다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// Unix 도메인 소켓 (권장). 경로는 `.sock` 으로 끝나야 합니다.
    Unix(PathBuf),
    /// loopback TCP.
    Tcp(SocketAddr),
}

impl Endpoint {
    /// `unix:<path>` 또는 `tcp:<ip>:<port>` 문자열을 파싱합니다.
    pub fn parse(s: &str) -> Result<Self> {
        if let Some(path) = s.strip_prefix("unix:") {
            if path.is_empty() {
                return Err(Error::Invalid("endpoint: empty unix socket path".into()));
            }
            return Ok(Self::Unix(PathBuf::from(path)));
        }
        if let Some(addr) = s.strip_prefix("tcp:") {
            let addr: SocketAddr = addr
                .parse()
                .map_err(|e| Error::Invalid(format!("endpoint: bad tcp address {addr:?}: {e}")))?;
            let ep = Self::Tcp(addr);
            ep.check_local()?;
            return Ok(ep);
        }
        Err(Error::Invalid(format!(
            "endpoint: expected `unix:<path>` or `tcp:<ip>:<port>`, got {s:?}"
        )))
    }

    fn check_local(&self) -> Result<()> {
        match self {
            Self::Unix(_) => Ok(()),
            Self::Tcp(addr) if addr.ip().is_loopback() => Ok(()),
            Self::Tcp(addr) => Err(Error::Invalid(format!(
                "endpoint: plaintext tcp to non-loopback {addr} refused; use a SecureChannel for remote engines"
            ))),
        }
    }

    async fn connect(&self) -> Result<IoStream> {
        self.check_local()?;
        match self {
            #[cfg(unix)]
            Self::Unix(path) => {
                let s = tokio::net::UnixStream::connect(path).await?;
                Ok(Box::pin(s))
            }
            #[cfg(not(unix))]
            Self::Unix(_) => Err(Error::Invalid(
                "endpoint: unix sockets are not supported on this platform".into(),
            )),
            Self::Tcp(addr) => {
                let s = tokio::net::TcpStream::connect(addr).await?;
                s.set_nodelay(true)?;
                Ok(Box::pin(s))
            }
        }
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unix(p) => write!(f, "unix:{}", p.display()),
            Self::Tcp(a) => write!(f, "tcp:{a}"),
        }
    }
}

/// 서버 확보 방식.
pub enum LaunchMode {
    /// 이미 실행 중인 서버에 접속.
    Attach {
        /// 접속 엔드포인트.
        endpoint: Endpoint,
        /// 서버가 요구하는 API 키 (있다면).
        api_key: Option<Zeroizing<String>>,
        /// 서버가 로드한 모델이 이 핸들과 같은 파일인지 `/props` 로 확인.
        /// `None` 이면 모델 검증을 건너뜁니다 (개발 전용, 경고 로그).
        expected_model: Option<VerifiedModelHandle>,
    },
    /// Lumen 이 직접 기동하고 감독.
    Spawn(SpawnSpec),
}

/// [`LlamaServerEngine`] 설정.
pub struct LlamaServerConfig {
    /// 서버 확보 방식.
    pub mode: LaunchMode,
    /// 모든 요청에 적용할 GBNF 문법 (도구 호출 제약). `None` = 자유 텍스트.
    pub grammar: Option<String>,
    /// 접속 + 응답 헤더 대기 / 스트림 유휴 타임아웃.
    pub request_timeout: Duration,
    /// 슬롯 프롬프트 캐시 재사용. 결정론이 중요하면 `false`.
    pub cache_prompt: bool,
}

impl LlamaServerConfig {
    /// 실행 중인 서버에 접속하는 설정.
    pub fn attach(endpoint: Endpoint) -> Self {
        Self {
            mode: LaunchMode::Attach {
                endpoint,
                api_key: None,
                expected_model: None,
            },
            grammar: None,
            request_timeout: Duration::from_secs(300),
            cache_prompt: false,
        }
    }

    /// 서버를 직접 기동하는 설정.
    pub fn spawn(spec: SpawnSpec) -> Self {
        Self {
            mode: LaunchMode::Spawn(spec),
            grammar: None,
            request_timeout: Duration::from_secs(300),
            cache_prompt: false,
        }
    }

    /// (attach 전용) API 키 설정.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        if let LaunchMode::Attach { api_key, .. } = &mut self.mode {
            *api_key = Some(Zeroizing::new(key.into()));
        }
        self
    }

    /// (attach 전용) `/props` 모델 경로 대조용 검증 핸들.
    pub fn expected_model(mut self, handle: VerifiedModelHandle) -> Self {
        if let LaunchMode::Attach { expected_model, .. } = &mut self.mode {
            *expected_model = Some(handle);
        }
        self
    }

    /// GBNF 문법 설정.
    pub fn grammar(mut self, grammar: Option<String>) -> Self {
        self.grammar = grammar;
        self
    }

    /// 요청 타임아웃 설정.
    pub fn request_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }

    /// 프롬프트 캐시 설정.
    pub fn cache_prompt(mut self, on: bool) -> Self {
        self.cache_prompt = on;
        self
    }

    /// 문자열 키/값 (정책 파일 `[inference.params]`) 으로부터 구성합니다.
    ///
    /// 허용 키: `mode` (`spawn` | `attach`), `endpoint`, `api_key_file`,
    /// `model`, `model_hash`, `model_name`, `model_version`, `binary`,
    /// `binary_hash`, `n_ctx`, `threads`, `gpu_layers`, `parallel`,
    /// `extra_args`, `startup_timeout_secs`, `request_timeout_secs`,
    /// `cache_prompt`. 그 외 키는 거부됩니다.
    pub fn from_params(params: &BTreeMap<String, String>) -> Result<Self> {
        const ALLOWED: &[&str] = &[
            "mode",
            "endpoint",
            "api_key_file",
            "model",
            "model_hash",
            "model_name",
            "model_version",
            "binary",
            "binary_hash",
            "n_ctx",
            "threads",
            "gpu_layers",
            "parallel",
            "extra_args",
            "startup_timeout_secs",
            "request_timeout_secs",
            "cache_prompt",
            "grammar",
        ];
        crate::backend::reject_unknown_params("llama-server", params, ALLOWED)?;
        let get = |k: &str| params.get(k).map(String::as_str);
        let req = |k: &str| {
            get(k).ok_or_else(|| Error::Invalid(format!("llama-server: missing param `{k}`")))
        };
        let parse_u32 = |k: &str| -> Result<Option<u32>> {
            get(k)
                .map(|v| {
                    v.parse::<u32>()
                        .map_err(|e| Error::Invalid(format!("llama-server: `{k}`: {e}")))
                })
                .transpose()
        };

        let endpoint = Endpoint::parse(req("endpoint")?)?;
        let model = match get("model") {
            Some(path) => {
                let hash: Blake3Hash = req("model_hash")?.parse()?;
                let path = Path::new(path);
                let name = get("model_name")
                    .map(str::to_owned)
                    .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "model".into());
                let version = get("model_version").unwrap_or("0").to_owned();
                Some(VerifiedModelLoader::hash_only().load_hash_only(
                    path,
                    hash,
                    name,
                    version,
                    lumen_provenance::Format::Gguf,
                )?)
            }
            None => None,
        };

        let mut cfg = match req("mode")? {
            "attach" => {
                let api_key = match get("api_key_file") {
                    Some(p) => Some(read_api_key_file(Path::new(p))?),
                    None => None,
                };
                Self {
                    mode: LaunchMode::Attach {
                        endpoint,
                        api_key,
                        expected_model: model,
                    },
                    grammar: None,
                    request_timeout: Duration::from_secs(300),
                    cache_prompt: false,
                }
            }
            "spawn" => {
                let model = model.ok_or_else(|| {
                    Error::Invalid(
                        "llama-server: spawn mode requires `model` + `model_hash`".into(),
                    )
                })?;
                let binary_hash: Blake3Hash = req("binary_hash")?.parse()?;
                let mut spec = SpawnSpec::new(req("binary")?, binary_hash, model, endpoint);
                if let Some(v) = parse_u32("n_ctx")? {
                    spec.n_ctx = v;
                }
                spec.n_threads = parse_u32("threads")?;
                spec.n_gpu_layers = parse_u32("gpu_layers")?;
                if let Some(v) = parse_u32("parallel")? {
                    spec.parallel = v.max(1);
                }
                if let Some(v) = parse_u32("startup_timeout_secs")? {
                    spec.startup_timeout = Duration::from_secs(u64::from(v));
                }
                if let Some(extra) = get("extra_args") {
                    spec.extra_args = extra.split_whitespace().map(str::to_owned).collect();
                }
                Self::spawn(spec)
            }
            other => {
                return Err(Error::Invalid(format!(
                    "llama-server: `mode` must be `spawn` or `attach`, got {other:?}"
                )))
            }
        };
        if let Some(v) = parse_u32("request_timeout_secs")? {
            cfg.request_timeout = Duration::from_secs(u64::from(v));
        }
        cfg.grammar = get("grammar").map(str::to_owned);
        if let Some(v) = get("cache_prompt") {
            cfg.cache_prompt = match v {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(Error::Invalid(
                        "llama-server: `cache_prompt` must be `true` or `false`".into(),
                    ))
                }
            };
        }
        Ok(cfg)
    }
}

fn read_api_key_file(path: &Path) -> Result<Zeroizing<String>> {
    let raw = Zeroizing::new(std::fs::read_to_string(path)?);
    let key = raw.trim();
    if key.is_empty() || key.contains(char::is_control) {
        return Err(Error::Invalid(format!(
            "llama-server: api key file {path:?} is empty or malformed"
        )));
    }
    Ok(Zeroizing::new(key.to_owned()))
}

/// `llama-server` 를 사용하는 추론 엔진.
pub struct LlamaServerEngine {
    endpoint: Endpoint,
    api_key: Option<Zeroizing<String>>,
    grammar: Option<String>,
    request_timeout: Duration,
    cache_prompt: bool,
    info: EngineInfo,
    process: Mutex<Option<LlamaServerProcess>>,
}

impl fmt::Debug for LlamaServerEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlamaServerEngine")
            .field("endpoint", &self.endpoint)
            .field("model", &self.info.model)
            .field("grammar", &self.grammar.is_some())
            .finish_non_exhaustive()
    }
}

impl LlamaServerEngine {
    /// 설정에 따라 서버를 기동하거나 접속하고, 준비 상태와 모델 경로를
    /// 확인한 뒤 엔진을 반환합니다.
    pub async fn from_config(config: LlamaServerConfig) -> Result<Self> {
        let LlamaServerConfig {
            mode,
            grammar,
            request_timeout,
            cache_prompt,
        } = config;

        let (endpoint, api_key, expected_model, process, startup_timeout) = match mode {
            LaunchMode::Attach {
                endpoint,
                api_key,
                expected_model,
            } => (endpoint, api_key, expected_model, None, request_timeout),
            LaunchMode::Spawn(spec) => {
                let key = fresh_api_key()?;
                let process = LlamaServerProcess::spawn(&spec, &key)?;
                (
                    spec.endpoint.clone(),
                    Some(key),
                    Some(spec.model.clone()),
                    Some(process),
                    spec.startup_timeout,
                )
            }
        };

        let mut engine = Self {
            endpoint,
            api_key,
            grammar,
            request_timeout,
            cache_prompt,
            info: EngineInfo {
                backend: "llama-server".into(),
                ..Default::default()
            },
            process: Mutex::new(process),
        };

        engine.wait_ready(startup_timeout).await?;
        let props: Props = engine.get_json("/props").await?;

        match (&expected_model, &props.model_path) {
            (Some(handle), Some(served)) => {
                let served_canon = Path::new(served)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(served));
                if served_canon != handle.path() {
                    return Err(Error::Provenance(format!(
                        "llama-server serves {served:?} but verified model is {:?}",
                        handle.path()
                    )));
                }
            }
            (Some(_), None) => {
                return Err(Error::Provenance(
                    "llama-server: /props has no model_path; cannot bind served model to verified handle".into(),
                ));
            }
            (None, _) => {
                tracing::warn!(
                    target: "lumen.inference.llama",
                    "llama-server attach without expected_model: served model is NOT bound to a verified handle"
                );
            }
        }

        let slots = props.total_slots.unwrap_or(0);
        engine.info = EngineInfo {
            backend: "llama-server".into(),
            model: expected_model
                .as_ref()
                .map(|h| h.model_info.name.clone())
                .or(props.model_path.clone()),
            capabilities: EngineCapabilities {
                streaming: true,
                seed_deterministic: slots == 1,
                grammar: true,
                logprobs: false,
                native_tool_calls: false,
            },
            max_context: props
                .default_generation_settings
                .as_ref()
                .and_then(|g| g.n_ctx),
        };
        tracing::info!(
            target: "lumen.inference.llama",
            endpoint = %engine.endpoint,
            model = ?engine.info.model,
            slots,
            build = ?props.build_info,
            "llama-server 준비 완료"
        );
        Ok(engine)
    }

    /// 접속 엔드포인트.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// 감독 중인 프로세스가 있으면 종료합니다. attach 모드에서는 no-op.
    pub async fn shutdown(&self) -> Result<()> {
        let proc = self
            .process
            .lock()
            .map_err(|_| Error::Inference("process lock poisoned".into()))?
            .take();
        if let Some(p) = proc {
            p.shutdown().await?;
        }
        Ok(())
    }

    async fn wait_ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(status) = self.process_exit_status()? {
                return Err(Error::Inference(format!(
                    "llama-server exited during startup: {status}"
                )));
            }
            match self.get_json::<Health>("/health").await {
                Ok(h) if h.status == "ok" => return Ok(()),
                Ok(_) | Err(_) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Inference(format!(
                    "llama-server at {} not ready within {timeout:?}",
                    self.endpoint
                )));
            }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
    }

    fn process_exit_status(&self) -> Result<Option<std::process::ExitStatus>> {
        let mut guard = self
            .process
            .lock()
            .map_err(|_| Error::Inference("process lock poisoned".into()))?;
        match guard.as_mut() {
            Some(p) => p.try_exit_status(),
            None => Ok(None),
        }
    }

    fn authorize(&self, req: Request) -> Request {
        match &self.api_key {
            Some(k) => req.header("Authorization", format!("Bearer {}", k.as_str())),
            None => req,
        }
    }

    async fn send(&self, req: Request) -> Result<Response> {
        let req = self.authorize(req);
        let fut = async {
            let io = self.endpoint.connect().await?;
            http::send(io, &req).await
        };
        tokio::time::timeout(self.request_timeout, fut)
            .await
            .map_err(|_| {
                Error::Inference(format!("llama-server: request timed out ({})", req.path))
            })?
    }

    async fn expect_ok(resp: Response) -> Result<Response> {
        if resp.status == 200 {
            return Ok(resp);
        }
        let status = resp.status;
        let body = resp.read_body().await.unwrap_or_default();
        let msg = serde_json::from_slice::<ErrorBody>(&body)
            .map(|e| e.error.message)
            .unwrap_or_else(|_| String::from_utf8_lossy(&body).chars().take(200).collect());
        let hint = if status == 401 {
            " (api key rejected)"
        } else {
            ""
        };
        Err(Error::Inference(format!(
            "llama-server: http {status}{hint}: {msg}"
        )))
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = Self::expect_ok(self.send(Request::get(path)).await?).await?;
        let body = resp.read_body().await?;
        serde_json::from_slice(&body)
            .map_err(|e| Error::Decode(format!("llama-server: {path} response: {e}")))
    }

    async fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let bytes = serde_json::to_vec(body).map_err(|e| Error::Decode(e.to_string()))?;
        let resp = Self::expect_ok(self.send(Request::post_json(path, bytes)).await?).await?;
        let body = resp.read_body().await?;
        serde_json::from_slice(&body)
            .map_err(|e| Error::Decode(format!("llama-server: {path} response: {e}")))
    }

    fn token_from_chunk(chunk: &CompletionChunk) -> Token {
        Token {
            id: if chunk.tokens.len() == 1 {
                Some(chunk.tokens[0])
            } else {
                None
            },
            text: chunk.content.clone(),
            logprob: None,
            finish_reason: chunk.finish_reason(),
        }
    }
}

#[async_trait]
impl InferenceEngine for LlamaServerEngine {
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion> {
        let req = CompletionRequest::from_params(
            prompt,
            params,
            false,
            self.cache_prompt,
            self.grammar.as_deref(),
        );
        let chunk: CompletionChunk = self.post_json("/completion", &req).await?;
        if chunk.truncated {
            tracing::warn!(target: "lumen.inference.llama", "prompt truncated to fit context");
        }
        let tool_call = parse_tool_call(&chunk.content);
        Ok(Completion {
            text: chunk.content,
            tool_call,
        })
    }

    fn info(&self) -> EngineInfo {
        self.info.clone()
    }
}

#[async_trait]
impl StreamingEngine for LlamaServerEngine {
    async fn stream_complete(&self, prompt: &str, params: &SamplingParams) -> Result<TokenStream> {
        let req = CompletionRequest::from_params(
            prompt,
            params,
            true,
            self.cache_prompt,
            self.grammar.as_deref(),
        );
        let bytes = serde_json::to_vec(&req).map_err(|e| Error::Decode(e.to_string()))?;
        let resp =
            Self::expect_ok(self.send(Request::post_json("/completion", bytes)).await?).await?;
        let sse = resp.into_sse();
        let idle = self.request_timeout;

        struct State {
            sse: SseReader,
            done: bool,
            idle: Duration,
        }

        let s = stream::unfold(
            State {
                sse,
                done: false,
                idle,
            },
            |mut st| async move {
                if st.done {
                    return None;
                }
                let next = tokio::time::timeout(st.idle, st.sse.next_data()).await;
                let data = match next {
                    Err(_) => {
                        st.done = true;
                        return Some((
                            Err(Error::Inference("llama-server: stream idle timeout".into())),
                            st,
                        ));
                    }
                    Ok(Err(e)) => {
                        st.done = true;
                        return Some((Err(e), st));
                    }
                    Ok(Ok(None)) => return None,
                    Ok(Ok(Some(d))) => d,
                };
                if let Ok(err) = serde_json::from_str::<ErrorBody>(&data) {
                    st.done = true;
                    return Some((
                        Err(Error::Inference(format!(
                            "llama-server: {}",
                            err.error.message
                        ))),
                        st,
                    ));
                }
                let chunk: CompletionChunk = match serde_json::from_str(&data) {
                    Ok(c) => c,
                    Err(e) => {
                        st.done = true;
                        return Some((
                            Err(Error::Decode(format!("llama-server: stream chunk: {e}"))),
                            st,
                        ));
                    }
                };
                let token = LlamaServerEngine::token_from_chunk(&chunk);
                if token.finish_reason.is_some() {
                    st.done = true;
                }
                Some((Ok(token), st))
            },
        );
        Ok(Box::pin(s))
    }
}

fn fresh_api_key() -> Result<Zeroizing<String>> {
    let mut raw = Zeroizing::new([0u8; 32]);
    lumen_core::OsRng.try_fill_bytes(raw.as_mut())?;
    Ok(Zeroizing::new(hex::encode(raw.as_ref())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use futures::StreamExt;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    /// 테스트용 llama-server 모형.
    #[derive(Clone)]
    struct MockServer {
        api_key: Option<String>,
        model_path: String,
        content: String,
        bodies: Arc<Mutex<Vec<String>>>,
        health_ok: bool,
    }

    impl MockServer {
        fn new(model_path: &str) -> Self {
            Self {
                api_key: Some("secret".into()),
                model_path: model_path.into(),
                content: r#"{"tool":"echo","args":{"text":"hi"}}"#.into(),
                bodies: Arc::new(Mutex::new(Vec::new())),
                health_ok: true,
            }
        }

        async fn handle<S: AsyncRead + AsyncWrite + Unpin>(self, mut s: S) {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let head_end = loop {
                let n = s.read(&mut tmp).await.unwrap();
                if n == 0 {
                    return;
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break p;
                }
            };
            let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
            let mut lines = head.lines();
            let req_line = lines.next().unwrap().to_owned();
            let path = req_line.split(' ').nth(1).unwrap().to_owned();
            let mut content_length = 0usize;
            let mut auth = None;
            for l in lines {
                let (k, v) = l.split_once(':').unwrap();
                match k.to_ascii_lowercase().as_str() {
                    "content-length" => content_length = v.trim().parse().unwrap(),
                    "authorization" => auth = Some(v.trim().to_owned()),
                    _ => {}
                }
            }
            let mut body = buf[head_end + 4..].to_vec();
            while body.len() < content_length {
                let n = s.read(&mut tmp).await.unwrap();
                body.extend_from_slice(&tmp[..n]);
            }
            let body = String::from_utf8(body).unwrap();

            let respond = |status: &str, ctype: &str, payload: &str| {
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\r\n{payload}",
                    payload.len()
                )
            };

            if path != "/health" {
                if let Some(expected) = &self.api_key {
                    if auth.as_deref() != Some(&format!("Bearer {expected}")) {
                        let payload = r#"{"error":{"code":401,"message":"Invalid API Key","type":"authentication_error"}}"#;
                        s.write_all(
                            respond("401 Unauthorized", "application/json", payload).as_bytes(),
                        )
                        .await
                        .unwrap();
                        return;
                    }
                }
            }

            match path.as_str() {
                "/health" => {
                    let out = if self.health_ok {
                        respond("200 OK", "application/json", r#"{"status":"ok"}"#)
                    } else {
                        respond(
                            "503 Service Unavailable",
                            "application/json",
                            r#"{"error":{"code":503,"message":"Loading model","type":"unavailable_error"}}"#,
                        )
                    };
                    s.write_all(out.as_bytes()).await.unwrap();
                }
                "/props" => {
                    let payload = format!(
                        r#"{{"model_path":"{}","total_slots":1,"default_generation_settings":{{"n_ctx":2048}},"build_info":"mock"}}"#,
                        self.model_path
                    );
                    s.write_all(respond("200 OK", "application/json", &payload).as_bytes())
                        .await
                        .unwrap();
                }
                "/completion" => {
                    self.bodies.lock().unwrap().push(body.clone());
                    let req: serde_json::Value = serde_json::from_str(&body).unwrap();
                    if req["stream"] == true {
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n")
                            .await
                            .unwrap();
                        let chunks = [
                            r#"{"content":"Hel","tokens":[11],"stop":false}"#.to_string(),
                            r#"{"content":"lo","tokens":[22],"stop":false}"#.to_string(),
                            r#"{"content":"!","tokens":[33],"stop":true,"stop_type":"limit","tokens_predicted":3}"#.to_string(),
                        ];
                        for c in chunks {
                            let ev = format!("data: {c}\n\n");
                            s.write_all(format!("{:x}\r\n{ev}\r\n", ev.len()).as_bytes())
                                .await
                                .unwrap();
                        }
                        s.write_all(b"0\r\n\r\n").await.unwrap();
                    } else {
                        let payload = serde_json::json!({
                            "content": self.content,
                            "tokens": [1, 2, 3],
                            "stop": true,
                            "stop_type": "eos",
                            "tokens_predicted": 3
                        })
                        .to_string();
                        s.write_all(respond("200 OK", "application/json", &payload).as_bytes())
                            .await
                            .unwrap();
                    }
                }
                _ => {
                    s.write_all(respond("404 Not Found", "text/plain", "nope").as_bytes())
                        .await
                        .unwrap();
                }
            }
            let _ = s.shutdown().await;
        }

        async fn listen_tcp(self) -> Endpoint {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                loop {
                    let (sock, _) = listener.accept().await.unwrap();
                    tokio::spawn(self.clone().handle(sock));
                }
            });
            Endpoint::Tcp(addr)
        }

        #[cfg(unix)]
        async fn listen_unix(self, path: &Path) -> Endpoint {
            let _ = std::fs::remove_file(path);
            let listener = tokio::net::UnixListener::bind(path).unwrap();
            tokio::spawn(async move {
                loop {
                    let (sock, _) = listener.accept().await.unwrap();
                    tokio::spawn(self.clone().handle(sock));
                }
            });
            Endpoint::Unix(path.to_owned())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumen-llama-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// GGUF 매직 + 버전만 있는 최소 파일로 검증 핸들을 만듭니다.
    fn fake_gguf(dir: &Path) -> VerifiedModelHandle {
        let path = dir.join("model.gguf");
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 24]);
        std::fs::write(&path, &bytes).unwrap();
        let hash = Blake3Hash::of(&bytes);
        VerifiedModelLoader::hash_only()
            .load_hash_only(&path, hash, "fake", "1", lumen_provenance::Format::Gguf)
            .unwrap()
    }

    #[tokio::test]
    async fn attach_complete_with_grammar_and_tool_call() {
        let dir = temp_dir("attach");
        let handle = fake_gguf(&dir);
        let mock = MockServer::new(&handle.path().to_string_lossy());
        let bodies = mock.bodies.clone();
        let ep = mock.listen_tcp().await;

        let cfg = LlamaServerConfig::attach(ep)
            .api_key("secret")
            .expected_model(handle)
            .grammar(Some("root ::= \"x\"".into()));
        let engine = LlamaServerEngine::from_config(cfg).await.unwrap();
        let info = engine.info();
        assert_eq!(info.backend, "llama-server");
        assert_eq!(info.model.as_deref(), Some("fake"));
        assert!(info.capabilities.streaming && info.capabilities.grammar);
        assert!(info.capabilities.seed_deterministic);
        assert_eq!(info.max_context, Some(2048));

        let params = SamplingParams {
            seed: Some(42),
            ..Default::default()
        };
        let c = engine.complete("call echo", &params).await.unwrap();
        let call = c.tool_call.expect("tool call parsed from content");
        assert_eq!(call.id.as_str(), "echo");
        assert_eq!(call.args_json, r#"{"text":"hi"}"#);

        let body: serde_json::Value = serde_json::from_str(&bodies.lock().unwrap()[0]).unwrap();
        assert_eq!(body["grammar"], "root ::= \"x\"");
        assert_eq!(body["seed"], 42);
        assert_eq!(body["stream"], false);
        assert_eq!(body["cache_prompt"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn streaming_yields_tokens_and_finish_reason() {
        let dir = temp_dir("stream");
        let handle = fake_gguf(&dir);
        let mock = MockServer::new(&handle.path().to_string_lossy());
        let ep = mock.listen_tcp().await;
        let engine = LlamaServerEngine::from_config(
            LlamaServerConfig::attach(ep)
                .api_key("secret")
                .expected_model(handle),
        )
        .await
        .unwrap();

        let mut stream = engine
            .stream_complete("hi", &SamplingParams::default())
            .await
            .unwrap();
        let mut tokens = Vec::new();
        while let Some(t) = stream.next().await {
            tokens.push(t.unwrap());
        }
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].id, Some(11));
        assert_eq!(tokens[0].text, "Hel");
        assert_eq!(
            tokens[2].finish_reason,
            Some(crate::FinishReason::MaxTokens)
        );
        let text: String = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(text, "Hello!");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn wrong_api_key_rejected() {
        let dir = temp_dir("auth");
        let handle = fake_gguf(&dir);
        let mock = MockServer::new(&handle.path().to_string_lossy());
        let ep = mock.listen_tcp().await;
        let err = LlamaServerEngine::from_config(
            LlamaServerConfig::attach(ep)
                .api_key("wrong")
                .expected_model(handle),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("401"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn served_model_mismatch_rejected() {
        let dir = temp_dir("mismatch");
        let handle = fake_gguf(&dir);
        let mock = MockServer::new("/somewhere/else.gguf");
        let ep = mock.listen_tcp().await;
        let err = LlamaServerEngine::from_config(
            LlamaServerConfig::attach(ep)
                .api_key("secret")
                .expected_model(handle),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Provenance(_)), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn not_ready_times_out() {
        let dir = temp_dir("notready");
        let handle = fake_gguf(&dir);
        let mut mock = MockServer::new(&handle.path().to_string_lossy());
        mock.health_ok = false;
        let ep = mock.listen_tcp().await;
        let err = LlamaServerEngine::from_config(
            LlamaServerConfig::attach(ep)
                .api_key("secret")
                .request_timeout(Duration::from_millis(600)),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("not ready"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_socket_roundtrip() {
        let dir = temp_dir("uds");
        let handle = fake_gguf(&dir);
        let mock = MockServer::new(&handle.path().to_string_lossy());
        let ep = mock.listen_unix(&dir.join("llama.sock")).await;
        let engine = LlamaServerEngine::from_config(
            LlamaServerConfig::attach(ep)
                .api_key("secret")
                .expected_model(handle),
        )
        .await
        .unwrap();
        let c = engine
            .complete("x", &SamplingParams::default())
            .await
            .unwrap();
        assert!(c.tool_call.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_detects_early_exit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("spawn");
        let handle = fake_gguf(&dir);
        let bin = dir.join("fake-llama-server");
        std::fs::write(&bin, b"#!/bin/sh\nexit 3\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let hash = Blake3Hash::of_file(&bin).unwrap();
        let mut spec = SpawnSpec::new(&bin, hash, handle, Endpoint::Unix(dir.join("s.sock")));
        spec.startup_timeout = Duration::from_secs(5);
        let err = LlamaServerEngine::from_config(LlamaServerConfig::spawn(spec))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("exited during startup"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_rejects_reserved_extra_args_and_bad_hash() {
        let dir = temp_dir("reserved");
        let handle = fake_gguf(&dir);
        let bin = dir.join("bin");
        std::fs::write(&bin, b"x").unwrap();
        let hash = Blake3Hash::of_file(&bin).unwrap();
        let key = Zeroizing::new("k".to_string());

        let mut spec = SpawnSpec::new(
            &bin,
            hash,
            handle.clone(),
            Endpoint::Unix(dir.join("a.sock")),
        );
        spec.extra_args = vec!["--api-key".into(), "leak".into()];
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        assert!(matches!(
            LlamaServerProcess::spawn(&spec, &key).unwrap_err(),
            Error::Invalid(_)
        ));

        let spec = SpawnSpec::new(
            &bin,
            Blake3Hash::of(b"other"),
            handle,
            Endpoint::Unix(dir.join("a.sock")),
        );
        assert!(matches!(
            LlamaServerProcess::spawn(&spec, &key).unwrap_err(),
            Error::Provenance(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn endpoint_parse_and_display() {
        assert_eq!(
            Endpoint::parse("unix:/run/llama.sock").unwrap(),
            Endpoint::Unix(PathBuf::from("/run/llama.sock"))
        );
        assert_eq!(
            Endpoint::parse("tcp:127.0.0.1:8080").unwrap().to_string(),
            "tcp:127.0.0.1:8080"
        );
        assert!(Endpoint::parse("tcp:10.0.0.5:8080").is_err());
        assert!(Endpoint::parse("http://x").is_err());
        assert!(Endpoint::parse("unix:").is_err());
    }

    #[test]
    fn config_from_params() {
        let dir = temp_dir("params");
        let handle = fake_gguf(&dir);
        let mut p = BTreeMap::new();
        p.insert("mode".into(), "attach".into());
        p.insert("endpoint".into(), "unix:/tmp/l.sock".into());
        p.insert("model".into(), handle.path().to_string_lossy().into_owned());
        p.insert("model_hash".into(), handle.model_info.hash.to_hex());
        p.insert("request_timeout_secs".into(), "9".into());
        p.insert("cache_prompt".into(), "true".into());
        let cfg = LlamaServerConfig::from_params(&p).unwrap();
        assert_eq!(cfg.request_timeout, Duration::from_secs(9));
        assert!(cfg.cache_prompt);
        assert!(matches!(
            cfg.mode,
            LaunchMode::Attach {
                expected_model: Some(_),
                ..
            }
        ));

        p.insert("bogus".into(), "1".into());
        assert!(LlamaServerConfig::from_params(&p).is_err());
        p.remove("bogus");
        p.insert("mode".into(), "spawn".into());
        assert!(
            LlamaServerConfig::from_params(&p).is_err(),
            "spawn needs binary"
        );
        p.insert("binary".into(), "/usr/bin/llama-server".into());
        p.insert("binary_hash".into(), Blake3Hash::of(b"x").to_hex());
        p.insert("n_ctx".into(), "8192".into());
        p.insert("extra_args".into(), "--flash-attn --mlock".into());
        let cfg = LlamaServerConfig::from_params(&p).unwrap();
        match cfg.mode {
            LaunchMode::Spawn(spec) => {
                assert_eq!(spec.n_ctx, 8192);
                assert_eq!(spec.extra_args, vec!["--flash-attn", "--mlock"]);
            }
            _ => panic!("expected spawn"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
