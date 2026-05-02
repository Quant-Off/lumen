//! 결정성을 기본으로 하는 wasmtime 설정.

use lumen_core::{Error, Result};
use wasmtime::{Config, Engine};

/// 샌드박스 설정 노브.
#[derive(Clone, Debug)]
pub struct SandboxConfig {
    /// 초기 fuel 예산. 각 `consume_fuel` 체크포인트가 1 단위 차감.
    pub fuel: u64,
    /// WASM 선형 메모리 페이지 (64 KiB) 의 최대 개수.
    pub memory_pages: u32,
    /// 스택 크기 (바이트).
    pub stack_size_bytes: usize,
    /// 비동기 지원 활성화 여부 (tokio 통합을 위해 기본 `true`).
    pub async_support: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            fuel: 10_000_000,
            memory_pages: 256, // 16 MiB
            stack_size_bytes: 512 * 1024,
            async_support: true,
        }
    }
}

impl SandboxConfig {
    /// 이 설정으로부터 wasmtime [`Engine`] 을 빌드합니다.
    ///
    /// 반환되는 엔진은 ZK 바인딩 에이전트에 적합한 결정성 실행으로 구성
    /// 됩니다:
    /// - SIMD off (NaN 재현성)
    /// - Threads off (선형 메모리 공유 없음)
    /// - Relaxed-SIMD off
    /// - NaN canonicalisation on
    /// - Fuel + epoch interruption 활성으로 hard CPU 한도
    pub fn deterministic_engine(&self) -> Result<Engine> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.async_support(self.async_support);
        // 결정성: SIMD 가 cross-vendor float drift 의 주요 원인입니다.
        config.wasm_simd(false);
        config.wasm_relaxed_simd(false);
        config.wasm_bulk_memory(true);
        config.wasm_multi_memory(false);
        config.cranelift_nan_canonicalization(true);
        config.max_wasm_stack(self.stack_size_bytes);
        // 참고: WASM threads, reference-types, relaxed-SIMD proposal 은 우리가
        // 의존하는 `wasmtime` feature set 에서 빌드 시점에 비활성화되므로
        // 여기서 토글할 수 없으며 그럴 필요도 없습니다.
        Engine::new(&config).map_err(|e| Error::Sandbox(format!("engine: {e}")))
    }
}
