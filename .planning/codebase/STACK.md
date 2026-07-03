---
last_mapped: dfba311a939388c5892574e844e73a7953796cdd
last_mapped_date: 2026-05-07
---

# Technology Stack

**Analysis Date:** 2026-05-07

## Languages

**Primary:**
- Rust 1.85 (Edition 2021) - Entire workspace; enforced by `rust-toolchain.toml`
- WebAssembly (WASM32) - Agent sandbox bytecode (wasm32-unknown-unknown target)

**Secondary:**
- Korean docstrings/comments - Project convention (see `CLAUDE.md`)

## Runtime

**Rust Toolchain:**
- Channel: 1.93.0 (synced with CI in `.github/docker/Dockerfile.ci`)
- Components: clippy, rustfmt
- Target: wasm32-unknown-unknown (for agent WASM compilation)
- Profile: minimal

**Package Manager:**
- Cargo (bundled with Rust toolchain)
- Lockfile: `Cargo.lock` (present, must be updated when dependencies change)
- Resolver: v2 (workspace resolver)

## Frameworks & Core Dependencies

**Async Runtime:**
- `tokio` 1.x - Multi-threaded async runtime with macros, sync channels, time, I/O, filesystem
  - Features: rt-multi-thread, macros, sync, time, io-util, fs
  - Location: workspace-level dependency in `[workspace.dependencies]`

**Serialization:**
- `serde` 1.x - Serialization framework with derive macros
- `serde_json` 1.x - JSON codec
- `postcard` 1.x - Binary serialization (no-heapless-cas, std only to avoid unmaintained atomic-polyfill)
- `toml` 0.8 - TOML parsing for manifests

**Cryptography (In-House via elib-k0-nt):**
- `elib-blake` (path: `../elib-k0-nt/blake` v1.0.0) - BLAKE3 hashing (replaces upstream blake3)
- `elib-ed25519` (path: `../elib-k0-nt/ed25519` v1.0.0) - EdDSA signing/verification (replaces ed25519-dalek)
- `elib-x25519` (path: `../elib-k0-nt/x25519` v1.0.0) - ECDH key agreement for crypto-channel feature (replaces x25519-dalek)
- `elib-aes` (path: `../elib-k0-nt/aes` v1.0.0) - AES-256-GCM encryption for crypto-channel feature (replaces aes-gcm)
- `elib-rng` (path: `../elib-k0-nt/rng` v1.0.0) - HashDRBG-SHA256 deterministic random generation (replaces rand/rand_core)
- `elib-constant-time` (path: `../elib-k0-nt/constant-time` v1.0.0) - Timing-attack-resistant comparisons (replaces subtle)
- `elib-zeroize` (path: `../elib-k0-nt/zeroize` v1.0.0) - Secure memory clearing

**WASM Sandbox:**
- `wasmtime` 44.x - WASM runtime with JIT compilation (Cranelift), async support, deterministic fuel/epoch control
  - Features: async, cranelift, runtime, addr2line, demangle
  - Security boundary: used exclusively in `lumen-sandbox`
  - Rationale: No viable pure-Rust replacement (wasmi too slow, wasm3 requires C)
  - Note: Unsafe code required for FFI; documented with `// SAFETY:` comments per clippy rules

**CLI & Configuration:**
- `clap` 4.x - Command-line argument parsing with derive macros
- `tracing` 0.1.x - Structured logging framework
- `tracing-subscriber` 0.3.x - Log formatting and filtering (fmt, env-filter features)

**Pattern Matching & Defense:**
- `aho-corasick` 1.x - Multi-pattern string matching (prompt injection detection)
- `regex` 1.x - Regular expression matching (jailbreak detection)
- `once_cell` 1.x - Lazy static initialization for compiled pattern sets
- `globset` 0.4.x - Path pattern matching for capability-gated file access

**Concurrency Utilities:**
- `parking_lot` 0.12.x - Fast synchronization primitives (Mutex, RwLock) with fair scheduling

**Model Provenance:**
- `safetensors` 0.7.x - Model file header inspection (read-only, no network dependencies)
  - Note: aligned with candle-onnx's same version to avoid dependency duplication

**Error Handling:**
- `thiserror` 2.x - Error type derivation
- `anyhow` 1.x - Generic error handling for CLI

**Utilities:**
- `hex` 0.4.x - Hexadecimal encoding/decoding
- `futures` 0.3.x - Async utilities and combinator traits

## Feature Flags

**lumen-inference** (`crates/lumen-inference/Cargo.toml`):
- `candle` (optional) - ONNX routing engine via candle-core + candle-onnx
  - Requires source vendoring in air-gapped environments (see `AIR-GAPPED.md`)
  - CPU-only (no GPU acceleration)
- `llama-cpp` (optional) - llama.cpp backend for full LLM text generation (v0.5 in-progress)

**lumen-fixed** (`crates/lumen-fixed/Cargo.toml`):
- `calibration` (optional) - Float-to-Q-format conversions for offline weight preparation
  - MUST NOT be enabled in deterministic proof-bound codepath
- `serde` (optional) - Serialization support for fixed-point numbers

**lumen-zkml** (`crates/lumen-zkml/Cargo.toml`):
- `ezkl` (optional) - ezkl proving system integration (stub; actual backend TBD at v1.0 milestone)
  - Note: `halo2` feature removed in v0.4 (see kernel-comp.md)

**lumen-channel** (`crates/lumen-channel/Cargo.toml`):
- `crypto-channel` (optional) - AES-GCM + x25519 encrypted channels
  - Depends on elib-aes, elib-x25519

**lumen-sdk** (`crates/lumen-sdk/Cargo.toml`):
- `alloc` (optional) - Enables String/Vec helper functions (requires agent-side global allocator)
- `macros` (optional) - Re-exports `#[lumen_agent]` proc-macro from lumen-sdk-macros

## Build Profiles

**Release Profile:**
```toml
[profile.release]
opt-level = 3           # Full optimization
lto = "thin"            # Thin Link-Time Optimization
codegen-units = 1       # Single codegen unit for maximum optimization
panic = "abort"         # Abort on panic (no unwinding)
strip = "symbols"       # Strip debug symbols
```

**Release-Debug Profile:**
- Inherits release settings but with `debug = true` and `strip = "none"`
- For debugging optimized binaries

## Workspace Lints

**Rust Lints (workspace-level):**
- `unsafe_code = "deny"` - Forbid unsafe blocks by default (overridden per crate: `lumen-sandbox`, `lumen-sdk`)
- `missing_docs = "warn"` - Require public item documentation
- `unreachable_pub = "warn"` - Warn on unnecessarily public items
- `rust_2018_idioms = "warn"` (priority -1)

**Clippy Lints (workspace-level):**
- `all = "warn"` (priority -1) - Enable all correctness/suspicion/style/complexity/performance groups (basis for CI -D warnings)
- `undocumented_unsafe_blocks = "warn"` - Require `// SAFETY:` comments on all unsafe blocks
- Pedantic group NOT enabled workspace-wide (too noisy; crates can opt-in locally)

**CI Enforcement:**
- `cargo clippy --workspace --all-targets -- -D warnings` (converts warnings to hard errors)
- `cargo fmt --all -- --check` (formatting checked; no auto-fix)

## Critical Dependencies Removed (v0.4 Air-Gapped Compatibility)

**Replaced:**
- `blake3` → `elib-blake` (cryptographically identical API)
- `ed25519-dalek` → `elib-ed25519` (keying strategy updated)
- `subtle` → `elib-constant-time` (constant-time comparisons)
- `rand`/`rand_core` → `elib-rng` (deterministic DRBG, requires RngCore adapter)
- `aes-gcm` → `elib-aes` (via crypto-channel feature)
- `x25519-dalek` → `elib-x25519` (via crypto-channel feature)

**Removed Entirely:**
- `halo2_proofs` / `ff` / `pasta_curves` - ZK proof circuit library (v0.4: mock prover uses BLAKE3 commitment only; no succinct ZK yet)
- `candle-transformers` - LLM text generation via Hugging Face models (too large for air-gapped; replaced by llama-cpp stub)
- `tokenizers` - HuggingFace tokenizer library (contains C Oniguruma regex; GGUF models have built-in vocab)

## Platform Requirements

**Development:**
- Rust 1.85+ (enforced by rust-toolchain.toml)
- WASM compilation support: `rustup target add wasm32-unknown-unknown`
- Cargo (bundled)

**Build Requirements:**
```bash
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

**Production / Deployment:**
- Linux kernel 5.10+ with TEE support (Intel TDX / AMD SEV-SNP) for host-side TEE inference
- WASM sandbox runs in Ring 3 userspace (not suitable for kernel Ring 0 without separate trusted process)
- Air-gapped environments: source vendoring via `cargo vendor` (see `AIR-GAPPED.md`)

## Workspace Members (v0.4.1)

| Crate | Purpose |
|-------|---------|
| `lumen-core` | Shared IDs, BLAKE3 hashing, Ed25519 wrappers, error types, time utilities |
| `lumen-fixed` | Q-format fixed-point arithmetic (deterministic quantization) |
| `lumen-capability` | Capability-based access control with signed permission tokens |
| `lumen-channel` | Typed async secure channels (AES-GCM/x25519 behind feature) |
| `lumen-provenance` | Model file provenance: BLAKE3 + Ed25519 verification + SBOM (safetensors parsing) |
| `lumen-defense` | Low-latency prompt injection / jailbreak detection (Aho-Corasick + regex) |
| `lumen-zkml` | ZK proving-system trait + mock BLAKE3 commitment prover (ezkl stub) |
| `lumen-inference` | InferenceEngine trait + verified loader + quantization config (candle/llama.cpp features) |
| `lumen-sandbox` | wasmtime WASM sandbox with deterministic config and capability-gated host imports |
| `lumen-agent` | Agent runtime: defense → infer → policy-check → tool-exec → ZK-bind routing |
| `lumen-orchestrator` | Multi-agent supervisor with channel-only inter-agent communication |
| `lumen-sdk` | WASM agent SDK: safe Rust wrappers for host imports (no_std compatible) |
| `lumen-sdk-macros` | Procedural macros for `#[lumen_agent]` annotation |
| `lumen-attestation` | TEE attestation document parsers (Intel TDX, AMD SEV-SNP) |
| `lumen-onchain` | On-chain verifier scaffold (EVM Solidity / Mina o1js) + deployment automation |
| `lumen-cli` | `lumen` command-line interface |

---

*Stack analysis: 2026-05-07*
