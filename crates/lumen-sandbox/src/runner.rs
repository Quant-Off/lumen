//! 최상위 샌드박스 러너.

use lumen_capability::Capability;
use lumen_core::{Error, Result};
use wasmtime::{Engine, Linker, Module, Store};

use crate::config::SandboxConfig;
use crate::host::HostState;
use crate::imports;

/// 샌드박스 핸들. 여러 모듈이 컴파일 캐시를 공유할 수 있도록 wasmtime
/// `Engine` 을 소유합니다.
pub struct Sandbox {
    engine: Engine,
    config: SandboxConfig,
}

impl Sandbox {
    /// 새 샌드박스 빌드.
    pub fn new(config: SandboxConfig) -> Result<Self> {
        let engine = config.deterministic_engine()?;
        Ok(Self { engine, config })
    }

    /// 내부 엔진 (테스트 / 오케스트레이션 용).
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// 모듈을 컴파일하고 실행합니다.
    ///
    /// `caps` 는 주어진 도구 이름에 대한 capability 를 생성합니다. 매 호출
    /// 마다 새로 해석되므로 호스트가 step 사이에 capability 를 회전할 수
    /// 있습니다.
    pub async fn run_module<F>(
        &self,
        wasm_bytes: &[u8],
        host_state: HostState,
        caps: F,
    ) -> Result<HostState>
    where
        F: Fn(&str) -> Option<Capability> + Send + Sync + 'static,
    {
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| Error::Sandbox(format!("compile: {e}")))?;
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        imports::register(&mut linker, caps)?;

        let mut store = Store::new(&self.engine, host_state);
        store
            .set_fuel(self.config.fuel)
            .map_err(|e| Error::Sandbox(format!("set_fuel: {e}")))?;
        store.set_epoch_deadline(1);

        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| Error::Sandbox(format!("instantiate: {e}")))?;
        if let Some(start) = instance.get_func(&mut store, "_start") {
            start
                .call_async(&mut store, &[], &mut [])
                .await
                .map_err(|e| Error::Sandbox(format!("_start: {e}")))?;
        }

        Ok(store.into_data())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_capability::PolicyEngine;
    use lumen_core::rng::OsRng;
    use lumen_core::{AgentId, SigningKey, Timestamp};

    const LOG_ONLY_WAT: &str = r#"
        (module
          (import "lumen" "lumen_log" (func $log (param i32 i32 i32)))
          (memory (export "memory") 1)
          (data (i32.const 100) "hi from wasm")
          (func (export "_start")
            (call $log (i32.const 1) (i32.const 100) (i32.const 12)))
        )
    "#;

    #[tokio::test]
    async fn log_only_module_records_audit() {
        let wasm = wat::parse_str(LOG_ONLY_WAT).unwrap();
        let policy = std::sync::Arc::new(PolicyEngine::new(vec![]));
        let agent = AgentId::random(&mut OsRng);
        let host = HostState::new(policy, agent).with_now(Timestamp::from_millis(0));
        let sandbox = Sandbox::new(SandboxConfig::default()).unwrap();
        let after = sandbox.run_module(&wasm, host, |_| None).await.unwrap();
        let audit = after.audit.lock();
        assert_eq!(audit.len(), 1);
        assert!(audit[0].contains("hi from wasm"), "got: {audit:?}");
    }

    #[tokio::test]
    async fn missing_capability_denies_tool() {
        let wat = r#"
            (module
              (import "lumen" "lumen_call_tool"
                (func $call (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "echo")
              (data (i32.const 16) "{}")
              (func (export "_start")
                (drop (call $call (i32.const 0) (i32.const 4) (i32.const 16) (i32.const 2)))))
        "#;
        let wasm = wat::parse_str(wat).unwrap();
        let _sk = SigningKey::generate(&mut OsRng);
        let policy = std::sync::Arc::new(PolicyEngine::new(vec![]));
        let agent = AgentId::random(&mut OsRng);
        let host = HostState::new(policy, agent).with_now(Timestamp::from_millis(0));
        let sandbox = Sandbox::new(SandboxConfig::default()).unwrap();
        let after = sandbox.run_module(&wasm, host, |_tool| None).await.unwrap();
        let calls = after.tool_calls.lock();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, "echo");
        assert!(!calls[0].authorised);
    }
}
