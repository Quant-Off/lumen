---
title: Lumen Coding Conventions
last_mapped_commit: dfba311a939388c5892574e844e73a7953796cdd
last_mapped_date: 2026-05-07
---

# Coding Conventions

**Analysis Date:** 2026-05-07

## Language & Edition

**Rust 2021 Edition**
- Minimum Supported Rust Version (MSRV): `1.85` (enforced in `Cargo.toml`)
- All crates compile with Rust 2021 edition semantics

## Safety & Lint Policy

### Workspace-wide Lints (Cargo.toml)

**Deny List:**
- `unsafe_code = "deny"` — Entire workspace forbids `unsafe` blocks
  - Every crate enforces `#![forbid(unsafe_code)]` in `src/lib.rs` or `src/main.rs`
  - Exception handling: Only elib cryptographic libraries (`elib-k0-nt`) may contain unsafe code; Lumen crates never do

**Warning List:**
- `missing_docs = "warn"` — All public items require documentation
  - Applies to all public types, functions, modules, traits
  - Korean docstrings (see below)
- `unreachable_pub = "warn"` — Flag unused public visibility
- `rust_2018_idioms = "warn"` (priority -1)
- `clippy::all = "warn"` (priority -1) — Core Clippy checks required
- `clippy::undocumented_unsafe_blocks = "warn"` — Even if `unsafe` exists elsewhere, block-level docs required

**CI Gate:**
- All Clippy warnings are treated as errors: `cargo clippy --workspace --all-targets -- -D warnings`
- `RUSTFLAGS: -D warnings` set globally in `.github/workflows/ci.yml`

## Documentation

**Language:** Korean (한국어)
- All docstrings and module docs written in Korean
- Example: `crates/lumen-core/src/lib.rs` — "Lumen 의 코어 프리미티브..."
- All comments explaining logic use Korean

**Doc Comment Format:**
```rust
/// 이 함수가 무엇을 하는지 한 줄 요약.
///
/// 더 상세한 설명이 필요하면 추가 단락으로 작성합니다.
///
/// ## 에러
/// 어떤 상황에서 실패하는지 설명.
///
/// ## 예시
/// ```rust
/// let result = my_function()?;
/// ```
pub fn my_function() -> Result<T> { }
```

**Module Header:**
```rust
//! 모듈 전체 목적과 구조.
//!
//! 주요 타입이나 패턴 설명.
```

## Naming Conventions

### Files & Directories

**Crate Names:**
- `lumen-*` prefix (kebab-case)
- Examples: `lumen-core`, `lumen-agent`, `lumen-sandbox`, `lumen-inference`
- Directory structure: `crates/lumen-*/src/`

**Module Files:**
- Snake_case: `capability.rs`, `error.rs`, `host.rs`, `runner.rs`
- Re-exports in `lib.rs`: `pub mod capability;`

### Types

**PascalCase** for all public types:
```rust
pub struct CapabilityBody { }
pub enum BackendConfig { }
pub trait InferenceEngine { }
pub struct AgentRuntime { }
```

### Functions & Methods

**snake_case** for all function and method names:
```rust
pub fn verify_signature(&self) -> Result<()> { }
pub async fn complete(&self, prompt: &str) -> Result<Completion> { }
pub fn build() -> AgentRuntime { }
```

### Variables & Constants

**snake_case** for local variables and function parameters:
```rust
let mut agent_id = AgentId::random(&mut rng);
let policy_engine = Arc::new(PolicyEngine::new(vec![]));
```

**SCREAMING_SNAKE_CASE** for constants:
```rust
const DEFAULT_TIMEOUT_MS: u64 = 5000;
```

### Error Variants

**PascalCase** - part of enum variant naming:
```rust
#[error("crypto: {0}")]
Crypto(String),

#[error("policy: {0}")]
Policy(String),

#[error("not implemented: {0}")]
NotImplemented(&'static str),
```

## Error Handling

### Error Type Strategy

**Unified Error Enum:**
- Defined in `crates/lumen-core/src/error.rs`
- Single `Error` enum with module-scoped variants
- `#[derive(Debug, Error)]` from `thiserror` crate
- Marked `#[non_exhaustive]` to allow future variants

**Common Variants:**
```rust
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),  // Auto-convert from std::io::Error

    #[error("crypto: {0}")]
    Crypto(String),

    #[error("policy: {0}")]
    Policy(String),

    #[error("not implemented: {0}")]
    NotImplemented(&'static str),

    // ... subsystem-specific variants
}

pub type Result<T> = std::result::Result<T, Error>;
```

### Mapping Subsystem Errors

**Pattern:**
1. Subsystems define native errors (e.g., `CryptoError`)
2. Map into `lumen_core::Error` at public API boundaries
3. Use `.map_err()` or `?` operator to lift errors
4. Prefer reusing existing variants over creating new ones

**Example (from `capability.rs`):**
```rust
pub fn sign(body: CapabilityBody, key: &SigningKey) -> Result<Self> {
    let bytes = postcard::to_allocvec(&body)
        .map_err(|e| Error::Decode(format!("capability body encode: {e}")))?;
    // ... rest of logic
}
```

### Result Usage

- Always return `Result<T>` from fallible functions
- Use `?` operator for error propagation
- `NotImplemented(&'static str)` for unfinished features
- Avoid generic error strings; use contextual `Error::*` variants

**Example:**
```rust
pub fn verify_model(path: &Path, manifest: &ModelManifest) -> Result<VerificationInfo> {
    let computed = Blake3Hash::of_file(path)?;
    if computed != manifest.hash {
        return Err(Error::Provenance("hash mismatch".into()));
    }
    Ok(VerificationInfo { /* ... */ })
}
```

## Async & Concurrency

### Async Runtime

**Tokio with Selective Features:**
- `tokio` v1 with features: `["rt-multi-thread", "macros", "sync", "time", "io-util", "fs"]`
- Multi-threaded async executor for runtime
- Default is async-first design

### Trait Bounds for Async

**async-trait Macro:**
```rust
use async_trait::async_trait;

#[async_trait]
pub trait InferenceEngine: Send + Sync {
    /// Completion execution.
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion>;
}
```

**Pattern:**
- All async traits use `#[async_trait]` from `async-trait` crate
- Trait methods return `Result<T>` not `impl Future`
- Concrete implementers use regular `async fn` inside trait impl

### Testing Async Code

- Use `#[tokio::test]` for async tests (see TESTING.md)
- Never use blocking operations in async contexts
- Use `Arc` for shared state in multi-agent scenarios

### Synchronization Primitives

**Preferred:**
- `Arc<T>` for thread-safe shared ownership
- `parking_lot::Mutex` for shared mutable state (faster than `std::sync::Mutex`)
- `tokio::sync::*` for async-aware primitives

**Example (from `tests/determinism.rs`):**
```rust
let policy = Arc::new(PolicyEngine::new(vec![vk]));
let runtime = AgentRuntime::builder(agent, policy).build()?;
```

## Code Organization

### Module Structure

**lib.rs Pattern:**
```rust
//! Module-level docstring in Korean.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod submodule1;
pub mod submodule2;

pub use submodule1::{Type1, Type2};
pub use submodule2::Type3;
```

**Barrel Re-exports:**
- Use re-exports in `lib.rs` for public API
- Keep internal modules private when possible
- Example: `pub use crypto::{Signature, SigningKey, VerifyingKey};`

### Function Size Guidelines

- Prefer functions under 50 lines
- Extracting builder patterns and setup into separate helpers
- Complex logic split into named helper functions

**Example (Agent Runtime):**
```rust
pub struct AgentRuntime { /* ... */ }

impl AgentRuntime {
    pub fn builder(agent: AgentId, policy: Arc<PolicyEngine>) -> RuntimeBuilder { }
    
    pub async fn step(&self, prompt: &str) -> Result<StepResult> {
        // Step through pipeline: Defense → Inference → Policy → Exec → ZK
    }
}
```

## Serialization

### Preferred Format

**postcard:**
- Binary serialization for capability tokens and wire formats
- `postcard::to_allocvec()` for encoding
- `postcard::from_bytes()` for decoding
- Map errors: `.map_err(|e| Error::Decode(format!(...)))?`

**serde with Derive:**
```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct MyType {
    pub field: String,
}
```

**JSON for config/debug:**
- `serde_json` for human-readable JSON
- Example: tool schema definitions use JSON

## Comments & Code Style

### When to Comment

- **Do:** Explain non-obvious design decisions
- **Do:** Document security-critical assumptions (capability gating, ZK bindings)
- **Do:** Note workarounds and version-specific issues
- **Do Not:** Repeat function signatures or obvious logic
- **Do Not:** Comment out dead code; delete it

### Comment Style

```rust
// Single-line comment for brief explanations

/* Multi-line comment rare; prefer stacked // lines instead */

// TODO / FIXME with context:
// TODO: Implement support for token refresh once EVM mainnet stable
```

### Security Comments

```rust
// SECURITY: This capability check is mandatory before any tool execution.
// Bypassing it would allow sandbox escape.
```

## Import Organization

### Import Order

```rust
// 1. Workspace crates (lumen-*)
use lumen_core::{Result, AgentId};
use lumen_capability::PolicyEngine;

// 2. External crates (alphabetical)
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

// 3. Re-exports from current crate (pub use)
pub use some_module::{PublicType};
```

### Path Aliases

No custom path aliases are configured. Use fully qualified paths:
- `use crate::submodule::Type;` for intra-crate
- `use lumen_core::Error;` for cross-workspace

## Formatting & Linting

### Formatting Tool

- **rustfmt** (implicit; no `.rustfmt.toml` — uses workspace defaults)
- Enforced in CI: `cargo fmt --all -- --check`
- Auto-format before commit: `cargo fmt --all`

### Lint Enforcement

- **clippy** with `-D warnings` in CI pipeline
- Run locally: `cargo clippy --workspace --all-targets -- -D warnings`
- Address all warnings before push

### Unsafe Code

- **Forbidden** in Lumen crates
- Only elib cryptographic dependencies (`elib-blake`, `elib-ed25519`, etc.) contain unsafe
- If unsafe is ever needed, must justify in comments and pass security review

## Build & Test Compliance

All code must pass the **4-gate CI pipeline** (from CLAUDE.md):

```bash
cargo build  --workspace --all-targets  # No compile errors
cargo test   --workspace                 # All tests pass
cargo clippy --workspace --all-targets -- -D warnings  # No Clippy warnings
cargo fmt    --all -- --check           # Consistent formatting
```

---

*Convention analysis: 2026-05-07*
