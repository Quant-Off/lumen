//! 추론 백엔드 팩토리와 확장 레지스트리.
//!
//! 두 가지 진입점을 제공합니다.
//!
//! - [`BackendConfig`] + [`build_engines`] : 이 크레이트에 내장된 백엔드를
//!   타입 안전하게 선택합니다. 변형은 개별 feature 로 보호됩니다.
//! - [`BackendRegistry`] : 이름 문자열로 팩토리를 등록하고 호출합니다. 외부
//!   크레이트 (예: 사내 vLLM / TensorRT-LLM 어댑터) 가 Lumen 코드를 수정하지
//!   않고 새 엔진을 올리는 확장점입니다. 내장 백엔드도 같은 이름으로
//!   미리 등록되어 있어 정책 파일의 `backend = "..."` 문자열 하나로 두
//!   경로를 통일할 수 있습니다.
//!
//! | 이름             | feature        | 설명                                     |
//! |------------------|----------------|------------------------------------------|
//! | `dummy`          | *(없음)*       | 결정론적 패턴 매칭, 테스트 전용.          |
//! | `llama-server`   | `llama-server` | 격리된 `llama-server` 프로세스 (llama.cpp) |
//!
//! `ChannelEngine` 은 `SecureChannel` 인스턴스를 필요로 하므로 팩토리가 아닌
//! [`crate::ChannelEngine::new`] 로 직접 생성합니다.

use std::collections::BTreeMap;
use std::sync::Arc;

use lumen_core::{Error, Result};

use crate::{DummyEngine, EngineInfo, InferenceEngine, StreamingEngine};

#[cfg(feature = "llama-server")]
use crate::llama::LlamaServerConfig;

/// 원하는 추론 백엔드와 설정을 기술하는 열거형.
#[non_exhaustive]
pub enum BackendConfig {
    /// 테스트 및 데모용 결정론적 패턴-매칭 엔진.
    Dummy,

    /// llama.cpp `llama-server` 백엔드.
    ///
    /// `llama-server` feature 가 활성화되어야 합니다. 설정에 따라 서버를
    /// 직접 기동 (바이너리 해시 검증 포함) 하거나 이미 실행 중인 소켓에
    /// 접속합니다.
    #[cfg(feature = "llama-server")]
    LlamaServer(Box<LlamaServerConfig>),
}

impl BackendConfig {
    /// 이 설정이 가리키는 백엔드의 레지스트리 이름.
    pub fn backend_name(&self) -> &'static str {
        match self {
            BackendConfig::Dummy => "dummy",
            #[cfg(feature = "llama-server")]
            BackendConfig::LlamaServer(_) => "llama-server",
        }
    }
}

/// 하나의 백엔드가 제공하는 trait 객체 묶음.
///
/// [`crate::InferenceEngine`] 은 필수이고, [`crate::StreamingEngine`] 은
/// 백엔드가 지원할 때만 `Some` 입니다. `AgentRuntimeBuilder::engines` 에
/// 그대로 전달하면 두 trait 이 함께 wiring 됩니다.
#[derive(Clone)]
pub struct Engines {
    /// completion 엔진.
    pub inference: Arc<dyn InferenceEngine>,
    /// 토큰 스트리밍 엔진 (지원 시).
    pub streaming: Option<Arc<dyn StreamingEngine>>,
}

impl std::fmt::Debug for Engines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engines")
            .field("info", &self.info())
            .field("streaming", &self.streaming.is_some())
            .finish()
    }
}

impl Engines {
    /// 스트리밍을 지원하지 않는 엔진으로 구성.
    pub fn non_streaming(inference: Arc<dyn InferenceEngine>) -> Self {
        Self {
            inference,
            streaming: None,
        }
    }

    /// 두 trait 을 모두 구현하는 단일 엔진으로 구성.
    pub fn from_dual<E>(engine: Arc<E>) -> Self
    where
        E: InferenceEngine + StreamingEngine + 'static,
    {
        Self {
            inference: engine.clone(),
            streaming: Some(engine),
        }
    }

    /// 백엔드 정보.
    pub fn info(&self) -> EngineInfo {
        self.inference.info()
    }
}

/// `config` 에 따라 내장 백엔드를 생성합니다.
///
/// 서버형 백엔드는 접속 / 기동에 I/O 가 필요하므로 `async` 입니다.
pub async fn build_engines(config: BackendConfig) -> Result<Engines> {
    match config {
        BackendConfig::Dummy => Ok(Engines::non_streaming(Arc::new(DummyEngine::new()))),

        #[cfg(feature = "llama-server")]
        BackendConfig::LlamaServer(cfg) => {
            let engine = crate::llama::LlamaServerEngine::from_config(*cfg).await?;
            Ok(Engines::from_dual(Arc::new(engine)))
        }
    }
}

/// `config` 에 따라 completion 엔진 trait 객체만 생성합니다.
///
/// 반환값은 `Arc<dyn InferenceEngine>` 이므로 `AgentRuntime` 에 바로 전달
/// 가능합니다. 스트리밍이 필요하면 [`build_engines`] 를 사용하세요.
pub async fn create_engine(config: BackendConfig) -> Result<Arc<dyn InferenceEngine>> {
    Ok(build_engines(config).await?.inference)
}

/// 이름 기반 백엔드 팩토리 시그니처.
///
/// `params` 는 백엔드별 자유 형식 키/값 (정책 파일의 `[inference.params]`)
/// 입니다. 팩토리는 자신이 이해하지 못하는 키를 **거부** 해야 합니다
/// (오타로 보안 옵션이 무시되는 사고 방지).
pub type BackendFactory = Arc<
    dyn Fn(
            BTreeMap<String, String>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Engines>> + Send + 'static>>
        + Send
        + Sync,
>;

/// 이름 -> 팩토리 레지스트리.
///
/// 순서와 조회가 결정론적이도록 `BTreeMap` 을 사용합니다. 같은 이름의
/// 중복 등록은 [`Error::Invalid`] 로 거부합니다 (조용한 덮어쓰기 금지).
#[derive(Clone, Default)]
pub struct BackendRegistry {
    factories: BTreeMap<String, BackendFactory>,
}

impl std::fmt::Debug for BackendRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

impl BackendRegistry {
    /// 빈 레지스트리.
    pub fn new() -> Self {
        Self::default()
    }

    /// 이 빌드에 컴파일된 내장 백엔드가 모두 등록된 레지스트리.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.register("dummy", |params| async move {
            reject_unknown_params("dummy", &params, &[])?;
            Ok(Engines::non_streaming(Arc::new(DummyEngine::new())))
        })
        .expect("fresh registry");

        #[cfg(feature = "llama-server")]
        reg.register("llama-server", |params| async move {
            let cfg = LlamaServerConfig::from_params(&params)?;
            build_engines(BackendConfig::LlamaServer(Box::new(cfg))).await
        })
        .expect("fresh registry");

        reg
    }

    /// 팩토리 등록.
    ///
    /// # Errors
    /// 이름이 비어 있거나 이미 등록되어 있으면 [`Error::Invalid`].
    pub fn register<F, Fut>(&mut self, name: &str, factory: F) -> Result<&mut Self>
    where
        F: Fn(BTreeMap<String, String>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Engines>> + Send + 'static,
    {
        if name.is_empty() {
            return Err(Error::Invalid("backend name must not be empty".into()));
        }
        if self.factories.contains_key(name) {
            return Err(Error::Invalid(format!(
                "backend `{name}` is already registered"
            )));
        }
        let boxed: BackendFactory = Arc::new(move |p| Box::pin(factory(p)));
        self.factories.insert(name.to_owned(), boxed);
        Ok(self)
    }

    /// 등록된 백엔드 이름 (정렬 순).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }

    /// 이름이 등록되어 있는지.
    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }

    /// `name` 백엔드를 `params` 로 생성합니다.
    ///
    /// # Errors
    /// 미등록 이름이면 [`Error::Invalid`], 그 외에는 팩토리의 에러.
    pub async fn build(&self, name: &str, params: &BTreeMap<String, String>) -> Result<Engines> {
        let factory = self.factories.get(name).ok_or_else(|| {
            Error::Invalid(format!(
                "unknown inference backend `{name}` (registered: {})",
                self.names().collect::<Vec<_>>().join(", ")
            ))
        })?;
        factory(params.clone()).await
    }
}

/// 팩토리 헬퍼: `allowed` 에 없는 키가 있으면 거부합니다.
pub fn reject_unknown_params(
    backend: &str,
    params: &BTreeMap<String, String>,
    allowed: &[&str],
) -> Result<()> {
    let unknown: Vec<&str> = params
        .keys()
        .map(String::as_str)
        .filter(|k| !allowed.contains(k))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "backend `{backend}`: unknown params {unknown:?} (allowed: {allowed:?})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SamplingParams;

    #[tokio::test]
    async fn builtin_dummy_via_registry() {
        let reg = BackendRegistry::with_builtins();
        assert!(reg.contains("dummy"));
        let engines = reg.build("dummy", &BTreeMap::new()).await.unwrap();
        assert!(engines.streaming.is_none());
        assert_eq!(engines.info().backend, "dummy");
        let c = engines
            .inference
            .complete("echo x", &SamplingParams::default())
            .await
            .unwrap();
        assert!(c.tool_call.is_some());
    }

    #[tokio::test]
    async fn unknown_backend_rejected() {
        let reg = BackendRegistry::with_builtins();
        let err = reg.build("nope", &BTreeMap::new()).await.unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }

    #[tokio::test]
    async fn unknown_params_rejected() {
        let reg = BackendRegistry::with_builtins();
        let mut p = BTreeMap::new();
        p.insert("typo".to_string(), "1".to_string());
        assert!(reg.build("dummy", &p).await.is_err());
    }

    #[tokio::test]
    async fn duplicate_registration_rejected() {
        let mut reg = BackendRegistry::with_builtins();
        let err = reg
            .register("dummy", |_| async { unreachable!() })
            .unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }

    #[tokio::test]
    async fn external_factory_registers_and_builds() {
        let mut reg = BackendRegistry::new();
        reg.register("my-engine", |_| async {
            Ok(Engines::non_streaming(Arc::new(DummyEngine::new())))
        })
        .unwrap();
        assert_eq!(reg.names().collect::<Vec<_>>(), vec!["my-engine"]);
        assert!(reg.build("my-engine", &BTreeMap::new()).await.is_ok());
    }
}
