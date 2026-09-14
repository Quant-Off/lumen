---
last_mapped: dfba311a939388c5892574e844e73a7953796cdd
last_mapped_date: 2026-05-07
---

# External Integrations

**Analysis Date:** 2026-05-07

## WASM Sandbox & Runtime

**wasmtime (v44.x):**
- What it's used for: Agent code isolation and deterministic execution
  - Location: `lumen-sandbox` crate (enforced in `crates/lumen-sandbox/Cargo.toml`)
  - Imports: `lumen-sandbox/src/runner.rs` - Module::new(), Store, Linker, async execution control
- Configuration:
  - JIT: Cranelift (runtime code generation for performance)
  - Fuel-based execution limits (deterministic step counting)
  - Epoch deadline enforcement (time-based timeout)
  - Address2line + demangle for error diagnostics
- Security Model:
  - Capabilities-based host import gating (`lumen-capability`)
  - Memory isolation via linear memory with explicit bounds
  - Export-only functions (no host function imports by default)
  - Host state passed via Store (no global mutable state)
- Production Readiness: Mature; used for production WASM execution across ecosystem

## Model Inference & Routing

**ONNX Routing (candle-onnx, optional feature: `candle`):**
- Service: ONNX model inference for tool routing decisions
  - Crates: `lumen-inference` (feature-gated in `crates/lumen-inference/Cargo.toml`)
  - Implementation: `crates/lumen-inference/src/candle.rs`, `crates/lumen-inference/src/backend.rs`
  - Model formats: ONNX (.onnx files)
  - Use case: Agent tool selection routing via neural network (argmax → tool ID mapping)
- SDK: candle-core (CPU tensor ops), candle-onnx (ONNX model loading)
- Deployment: Air-gapped environments require source vendoring via `cargo vendor` (see `AIR-GAPPED.md`)
- Model Loading:
  - Verified by `lumen-provenance` (BLAKE3 + Ed25519 signature check before loading)
  - Output: `VerifiedModelHandle` passed to inference engine
  - Safetensors header inspection supported

**LLM Inference (llama.cpp backend, optional feature: `llama-cpp`):**
- Service: Full LLM text generation via llama.cpp
  - Status: Stub interface in v0.4; full implementation targeting v0.5
  - Location: `crates/lumen-inference/src/llama_cpp.rs`
  - Integration: C FFI bindings (not yet materialized)
- Use case: Agent prompt reasoning when ONNX routing is insufficient
- Model support: GGUF format (built-in tokenizer, no separate tokenizer dependency)
- Note: Supersedes removed `candle-transformers` + `tokenizers` stack for text generation
- TODO: Implement C bindings and integration tests

**Dummy Engine (test/demo):**
- Location: `crates/lumen-inference/src/lib.rs`
- Purpose: Deterministic pattern-matching inference for tests and demos (no model dependencies)

## Secure Channel & Cryptographic Communication

**Encrypted Channels (feature: `crypto-channel`):**
- What: End-to-end encrypted async message channels
  - Location: `lumen-channel` crate
  - Implementation: `crates/lumen-channel/src/encrypted.rs`
  - Used by: Multi-agent orchestration via `lumen-orchestrator`
- Cryptographic Stack:
  - Key Agreement: x25519 ECDH (`elib-x25519`)
  - Encryption: AES-256-GCM (`elib-aes`)
  - Key Derivation: BLAKE3 keyed hash from shared secret (`elib-blake`)
  - Handshake Signature: Ed25519 (`elib-ed25519`)
  - Nonce: Incremental counter (replay prevention)
- Transport: in-process channel (tokio channels)
- Attestation: Each channel endpoint attested via signed certificates
  - Attestation data format: `lumen-attestation` wire format parsers
  - Intel TDX / AMD SEV-SNP document support (wiring in v0.4+)

## Model Provenance & Supply-Chain Security

**Provenance Verification Module:**
- What: BLAKE3 hash + Ed25519 signature verification for model files
  - Location: `crates/lumen-provenance/`
  - Core API: `verify_model()` (blocks loading on any mismatch)
  - Manifest format: `ModelManifest` (JSON/TOML serialized, signed variant `SignedManifest`)
- Supported Formats:
  - ONNX models (`.onnx`) - header sniffed via `crates/lumen-provenance/src/onnx.rs`
  - Safetensors (`.safetensors`) - header parsing via `safetensors` crate + custom inspection
  - GGUF models (`.gguf`) - stub parser in `crates/lumen-provenance/src/gguf.rs`
- Signature Verification:
  - Signing algorithm: Ed25519 (via `elib-ed25519`)
  - Signer identity: Verifying key pinned in deployment manifest
  - Certificate chain: Single-level (key → model hash)
- SBOM Generation:
  - CycloneDX-compatible software bill of materials (`crates/lumen-provenance/src/sbom.rs`)
  - Captured at model load time, exported via `generate_sbom()`
  - Includes: model name, hash, signer identity, timestamp
- Pinset Validation:
  - `PinSet` - Immutable set of approved (hash, signer) pairs
  - `PinAcceptance` - Allow/Deny/Suspect result codes
  - `RetiredPin` - Signature-invalid or compromised hashes can be retired

## Agent Sandboxing & Capability-Based Access Control

**Capability Framework:**
- What: Signed permission tokens for resource access
  - Location: `crates/lumen-capability/`
  - Core types: `Capability`, `CapabilityId`, capability policies
- Enforced Access:
  - File operations: Glob patterns matching (via `globset`)
  - Network operations: Hostname/port allowlisting (capability-gated import)
  - Tool execution: Whitelisted tool names (capability token for each)
  - System resources: Explicit per-capability grants
- Replay Prevention:
  - Nonce-based challenge-response (per-invocation freshness)
  - Timestamp validation (expiry enforcement)
  - Signature: Ed25519 over capability bytes
- Integration with WASM:
  - `lumen-sandbox` gates host imports to capabilities present in Store
  - Capabilities re-validated on each `call_async()` invocation

**Defense Module (prompt injection / jailbreak detection):**
- What: Real-time threat detection with minimal latency
  - Location: `crates/lumen-defense/`
  - Engine: Aho-Corasick multi-pattern matcher + RegexSet
- Detection Patterns:
  - Prompt injection markers (e.g., "ignore previous instructions", SQL keywords)
  - Known jailbreak payloads (curated set)
  - Heuristic patterns (unusual control characters, excessive Unicode)
- Integration:
  - Called by `lumen-agent` before inference (defense → infer → policy-check pipeline)
  - Blocks malicious prompts before reaching LLM
- Performance: Sub-millisecond latency via pre-compiled DFAs

## Deterministic Control & Fixed-Point Arithmetic

**Fixed-Point Math (lumen-fixed):**
- What: Q-format quantization for deterministic proof-bound arithmetic
  - Location: `crates/lumen-fixed/`
  - Use case: ZK proof generation requires hardware-independent computation
  - Feature: `calibration` - offline float-to-Q conversion (MUST NOT be used in proof path)
- Formats Supported:
  - Q15 (1 sign + 15 integer bits)
  - Q16 (1 sign + 16 integer bits)
  - Q24 (1 sign + 24 integer bits)
  - Custom precision via generic parameters
- Integration: LLM model weight quantization pipeline
  - Offline: candle-onnx float32 weights → Q-format (uses `calibration` feature)
  - Runtime: Quantized models loaded, deterministic inference executed

**Deterministic Agent Runtime (lumen-agent):**
- What: State machine execution with no floating-point, no randomness in main path
  - Location: `crates/lumen-agent/`
  - Pipeline: defense (regex) → inference (ONNX or dummy) → policy check (ZK proof) → tool exec → routing decision binding
  - State isolation: No shared mutable state; async isolation via tokio channels
  - Execution guarantee: Same input + same seed → identical output (for ZK verification)

## ZK Proving System & Circuit Logic

**Mock Prover (lumen-zkml):**
- What: Proof generation for routing decisions (v0.4: mock-only, witness exposed)
  - Location: `crates/lumen-zkml/`
  - Feature: `ezkl` (optional; stub for real proving system integration)
  - Implementation: `crates/lumen-zkml/src/mock.rs`
- Proof Subject: Agent tool-routing decision constraints
  - Witness: Prompt embedding, tool scores, selection threshold
  - Constraint: `selected_tool_score >= threshold && selected_tool_score > all_other_scores`
  - Commitment: BLAKE3 hash of witness (only publicly revealed commitment)
- Removed Dependencies (v0.4):
  - `halo2_proofs` - Mock prover replaced with direct assertion checking
  - `ff` / `pasta_curves` - Elliptic curve ops no longer needed for mock
- Future Milestone (v1.0):
  - Real succinct ZK backend (SP1 / RISC Zero / Aleo)
  - Proof sizes: ~200-300 bytes (estimated)
  - Verification gas cost: EVM-compatible (~200K gas for batch verification)

## Attestation & TEE Support

**TEE Attestation Parsers (lumen-attestation):**
- What: Intel TDX and AMD SEV-SNP attestation document wire format parsing
  - Location: `crates/lumen-attestation/`
  - Status: Wire format parsing only (v0.4); cryptographic validation deferred to v0.5+
  - Formats:
    - Intel TDX attestation quote + token (ECDSA-based)
    - AMD SEV-SNP attestation report (signed attestation)
- Use Case:
  - Host-side TEE attestation: Proof that LLM inference runs in trusted execution environment
  - Agent-side validation: Verify attestation before sending sensitive prompts to TEE
- Integration Path:
  - Secure channel (`lumen-channel` encrypted channels) between agent (WASM) and host TEE
  - Attestation certificates pinned in agent capability set
  - Ongoing: Cryptographic signature validation in v0.5

## On-Chain Verification & Deployment

**On-Chain Verifier Scaffolding (lumen-onchain):**
- What: Automated smart contract generation and deployment for proof verification
  - Location: `crates/lumen-onchain/`
  - Targets:
    - EVM (Solidity): Bytecode generation for proof verification circuits
    - Mina (o1js): Zero-knowledge DSL for ZK proof verification
  - Use Case:
    - Off-chain ZK proof generation (agent routing)
    - On-chain submission + verification (immutable audit trail)
    - Settlement: Confirmed proofs unlock fund transfer or policy enforcement
- Proof Format:
  - Input: Proving system circuit (from `lumen-zkml`)
  - Output: Solidity verifier contract OR Mina o1js verification circuit
  - Integration: Manual deployment flow (v0.4; automated in v0.5+)
- Example: "Agent was authorized to access resource X based on approved ZK proof Y"

## Logging & Audit

**Structured Logging (tracing):**
- Framework: `tracing` + `tracing-subscriber`
- Usage: Audit trail for all agent decisions, model loads, capability grants
  - Location: Used throughout workspace (e.g., `lumen-agent`, `lumen-provenance`)
  - Output: Console formatter with environment filter (`fmt`, `env-filter` features)
- Filtering: `RUST_LOG` environment variable (e.g., `lumen_agent=debug,lumen_provenance=info`)

## Build & Deployment Environment

**Vendoring for Air-Gapped Environments:**
- Tool: `cargo vendor` (standard Cargo command)
- When Required:
  - `candle` feature enabled (large dep tree, includes HF Hub examples)
  - All workspace dependencies (for deployment in closed networks)
- How: `cargo vendor --locked > vendors.tar.gz` (commit sources)
- Guide: `AIR-GAPPED.md` in repository root

**Continuous Integration:**
- CI Pipeline: `.github/docker/Dockerfile.ci` (Docker image with Rust 1.93.0)
- Build Checks:
  ```bash
  cargo build --workspace --all-targets
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  ```
- Enforcement: All four commands must pass for merge

## Environment Configuration

**Required Environment Variables:**
- None at runtime (all configs are manifest-based or CLI-provided)
- Development:
  - `RUST_LOG` - Logging filter (optional, defaults to info)
  - `RUST_BACKTRACE` - Backtrace verbosity (optional)

**Configuration Files:**
- Manifest format: JSON or TOML (parsed by `serde` + `serde_json` / `toml`)
  - Model manifests: `ModelManifest` type in `lumen-provenance`
  - Capability policies: `Policy` serialized via `postcard` or JSON
  - CLI config: `lumen.toml` (optional, parsed by `clap`)
- No secrets in environment (all cryptographic keys loaded from manifest or stored in TEE)

## External API / Network Integrations

**None Exposed (Design Principle):**
- No HTTP client dependencies (no `reqwest`, `hyper`)
- No cloud SDK integrations (no AWS, GCP, Azure SDKs)
- No external service calls from agent runtime
- TEE Integration: Channels only (no REST/gRPC)
- On-chain: Manual smart contract deployment (scaffolding provided, but no automated chain interaction)

**Rationale:** Zero-trust, air-gapped deployment model requires all I/O to be explicit and capability-gated.

## Build Artifacts & Performance

**Binary Output:**
- CLI binary: `target/release/lumen` (~15-20 MB with symbols, ~3-5 MB stripped)
- Library artifacts: `target/release/liblumen*.rlib` / `liblumen*.so`
- WASM agent modules: Compiled with `wasm32-unknown-unknown` target

**Optimization Profile (Release):**
- Optimization level: 3 (maximal)
- LTO: thin (balances speed + link-time cost)
- Panic: abort (smaller binary, no unwinding overhead)
- Debug symbols: stripped by default (use `release-debug` profile for debugging)

---

*Integration audit: 2026-05-07*
