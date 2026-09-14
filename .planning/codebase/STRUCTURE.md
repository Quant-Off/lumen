---
last_mapped_commit: 7c6fc8c
analysis_date: 2026-05-07
focus: structure
---

# Codebase Structure

**Analysis Date:** 2026-05-07

## Directory Layout

```
lumen/
├── crates/                              # Workspace members (15 crates)
│   ├── lumen-core/
│   │   ├── src/
│   │   │   ├── lib.rs                   # Core exports
│   │   │   ├── ids.rs                   # AgentId, CapabilityId, RequestId, ToolId
│   │   │   ├── crypto.rs                # Ed25519 wrappers (SigningKey, VerifyingKey)
│   │   │   ├── hash.rs                  # Blake3Hash + constant-time comparison
│   │   │   ├── rng.rs                   # RNG trait + HashDrbg, OsRng adapters
│   │   │   ├── error.rs                 # Error enum, Result type
│   │   │   ├── time.rs                  # Timestamp
│   │   │   └── Cargo.toml
│   │   └── tests/
│   │
│   ├── lumen-fixed/
│   │   ├── src/
│   │   │   ├── lib.rs                   # Q16_16, Q8_24 exports
│   │   │   ├── q16_16.rs                # Q-format fixed-point (range ±2^15)
│   │   │   ├── q8_24.rs                 # Q-format fixed-point (range ±2^7, precise)
│   │   │   ├── quant.rs                 # Quantization helpers (i8 quantize)
│   │   │   └── Cargo.toml
│   │   └── tests/
│   │
│   ├── lumen-capability/
│   │   ├── src/
│   │   │   ├── lib.rs                   # Capability, PolicyEngine, Action, Resource exports
│   │   │   ├── capability.rs            # Capability struct, signing, verification
│   │   │   ├── policy.rs                # PolicyEngine (verify, replay defense, audit)
│   │   │   ├── resource.rs              # Resource enum (FsRead, FsWrite, Net, CallTool, etc)
│   │   │   ├── audit.rs                 # Audit logging (deterministic field names)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── policy_enforcement.rs    # Policy check tests
│   │   └── examples/
│   │
│   ├── lumen-channel/
│   │   ├── src/
│   │   │   ├── lib.rs                   # SecureChannel trait
│   │   │   ├── inproc.rs                # InProcChannel (tokio mpsc)
│   │   │   ├── attested.rs              # AttestedChannel (Ed25519 handshake + frame signing)
│   │   │   ├── encrypted.rs             # EncryptedChannel (AES-256-GCM + x25519, feature-gated)
│   │   │   ├── tee.rs                   # TeeChannel stub (unimplemented)
│   │   │   ├── codec.rs                 # postcard encode/decode
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── roundtrip.rs             # Serialization tests
│   │   └── examples/
│   │
│   ├── lumen-defense/
│   │   ├── src/
│   │   │   ├── lib.rs                   # DefenseEngine, Verdict, BlockReason exports
│   │   │   ├── engine.rs                # DefenseEngine::analyze() pipeline
│   │   │   ├── lexicon.rs               # Aho-Corasick jailbreak corpus (30+ triggers)
│   │   │   ├── regex_stage.rs           # RegexSet for pattern matching
│   │   │   ├── heuristics.rs            # Heuristic scoring (non-printable, base64, length)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── verdicts.rs              # Defense verdict tests
│   │   └── examples/
│   │
│   ├── lumen-provenance/
│   │   ├── src/
│   │   │   ├── lib.rs                   # ModelManifest, verify_model, SBOM exports
│   │   │   ├── manifest.rs              # ModelManifest, SignedManifest, Format enum
│   │   │   ├── verify.rs                # verify_model() hash + signature validation
│   │   │   ├── gguf.rs                  # GGUF header sniffing
│   │   │   ├── onnx.rs                  # ONNX header sniffing
│   │   │   ├── safetensors_check.rs     # Safetensors header validation
│   │   │   ├── sbom.rs                  # CycloneDX SBOM generation
│   │   │   ├── pinset.rs                # Pin acceptance lists (retired pins)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── verify_safetensors.rs    # Safetensors verification tests
│   │   └── examples/
│   │
│   ├── lumen-zkml/
│   │   ├── src/
│   │   │   ├── lib.rs                   # ProvingSystem trait, Verification exports
│   │   │   ├── mock.rs                  # MockCommitmentProver (BLAKE3 commitment)
│   │   │   ├── verification.rs          # Verification enum (CommitmentOnly, ZkVerified, Invalid)
│   │   │   ├── ezkl.rs                  # ezkl stub (feature-gated)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── routing_proof.rs         # Mock proof generation tests
│   │   └── examples/
│   │
│   ├── lumen-inference/
│   │   ├── src/
│   │   │   ├── lib.rs                   # InferenceEngine trait, SamplingParams, Completion, ToolCall
│   │   │   ├── backend.rs               # BackendConfig, create_engine() factory
│   │   │   ├── dummy.rs                 # DummyEngine (deterministic test responses)
│   │   │   ├── candle.rs                # CandleEngine (ONNX routing, feature: candle)
│   │   │   ├── llama_cpp.rs             # LlamaCppEngine stub (feature: llama-cpp)
│   │   │   ├── loader.rs                # VerifiedModelLoader, VerifiedModelHandle
│   │   │   ├── quantize.rs              # QuantizationConfig, GgufLevel, QuantizationKind
│   │   │   ├── streaming.rs             # StreamingEngine trait, Token, TokenStream, FinishReason
│   │   │   ├── tee_channel.rs           # ChannelEngine (TEE forward via SecureChannel)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── engine_selection.rs      # Backend selection tests
│   │   └── examples/
│   │
│   ├── lumen-sandbox/
│   │   ├── src/
│   │   │   ├── lib.rs                   # Sandbox, SandboxConfig, HostState exports
│   │   │   ├── runner.rs                # Sandbox::run_module() execution
│   │   │   ├── config.rs                # SandboxConfig::deterministic_engine()
│   │   │   ├── host.rs                  # HostState (policy, agent, audit, tool_calls)
│   │   │   ├── imports.rs               # Host import implementations (lumen_log, lumen_call_tool)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── wasm_echo_agent.rs       # WASM agent test (determinism check)
│   │   └── examples/
│   │
│   ├── lumen-agent/
│   │   ├── src/
│   │   │   ├── lib.rs                   # AgentRuntime, StepResult, ToolRegistry exports
│   │   │   ├── runtime.rs               # AgentRuntime::step() / stream_step() pipeline
│   │   │   ├── tool.rs                  # Tool trait, ToolHandler, ToolRegistry
│   │   │   ├── route.rs                 # RoutingDecision, RoutingPublicInputs, RoutingWitness
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   ├── determinism.rs           # Determinism checks (same input → same output)
│   │   │   └── determinism_stress.rs    # Stress test for determinism
│   │   └── examples/
│   │
│   ├── lumen-orchestrator/
│   │   ├── src/
│   │   │   ├── lib.rs                   # Orchestrator, AgentHandle, InterAgentMessage exports
│   │   │   ├── supervisor.rs            # Orchestrator (registry, inboxes, message routing)
│   │   │   └── Cargo.toml
│   │   ├── tests/
│   │   │   └── multi_agent.rs           # Multi-agent execution tests
│   │   └── examples/
│   │
│   ├── lumen-sdk/
│   │   ├── src/
│   │   │   ├── lib.rs                   # log(), call_tool(), LogLevel, ToolError, no_std
│   │   │   ├── alloc_helpers.rs         # alloc-only helpers (feature: alloc)
│   │   │   └── Cargo.toml
│   │   └── examples/
│   │       └── hello_agent.rs           # Minimal WASM agent example
│   │
│   ├── lumen-sdk-macros/
│   │   ├── src/
│   │   │   ├── lib.rs                   # #[lumen_agent] proc-macro definition
│   │   │   └── Cargo.toml
│   │   └── tests/
│   │
│   ├── lumen-attestation/
│   │   ├── src/
│   │   │   ├── lib.rs                   # AttestationDoc, parse() exports
│   │   │   ├── tdx.rs                   # Intel TDX Quote v4 parser
│   │   │   ├── sev_snp.rs               # AMD SEV-SNP Attestation Report v2 parser
│   │   │   └── Cargo.toml
│   │   └── tests/
│   │
│   ├── lumen-onchain/
│   │   ├── src/
│   │   │   ├── lib.rs                   # Chain enum, VerifierMeta, emit/deploy exports
│   │   │   ├── emit.rs                  # emit_artifacts() (Solidity + o1js)
│   │   │   ├── deploy.rs                # deploy_evm(), deploy_mina() (child processes)
│   │   │   ├── evm.rs                   # Solidity RoutingVerifier.sol codegen
│   │   │   ├── mina.rs                  # o1js RoutingVerifier.ts codegen
│   │   │   └── Cargo.toml
│   │   └── examples/
│   │
│   └── lumen-cli/
│       ├── src/
│       │   ├── main.rs                  # CLI entry, subcommand dispatch
│       │   ├── policy_file.rs           # Policy TOML loading / validation
│       │   ├── cmd/
│       │   │   ├── init.rs              # `lumen init` — emit policy template
│       │   │   ├── verify_model.rs      # `lumen verify-model` — check BLAKE3 + sig
│       │   │   ├── sbom.rs              # `lumen sbom` — emit CycloneDX SBOM
│       │   │   ├── defend.rs            # `lumen defend` — test prompt defense
│       │   │   ├── prove.rs             # `lumen prove` — generate routing proof
│       │   │   ├── run.rs               # `lumen run` — end-to-end agent step
│       │   │   └── verifier.rs          # `lumen verifier deploy` — emit+deploy contracts
│       │   └── Cargo.toml
│       ├── tests/
│       │   └── e2e.rs                   # End-to-end CLI tests
│       └── examples/
│
├── agents/                              # (excluded from workspace; example WASM agents)
│   └── echo-agent/                      # Sample WASM agent using lumen-sdk
│       ├── src/
│       │   └── lib.rs                   # Agent logic (step fn)
│       ├── Cargo.toml                   # target: wasm32-unknown-unknown
│       └── build.sh                     # wasm compilation + linking
│
├── Cargo.toml                           # Workspace root, workspace dependencies, lints
├── Cargo.lock                           # Locked dependency versions
├── CLAUDE.md                            # Project-specific instructions (Korean)
├── kernel-comp.md                       # Air-gap compatibility audit (detailed dependency review)
├── README.md                            # Project overview
├── .github/
│   └── workflows/                       # CI/CD (linter, tests, clippy, fmt)
└── .planning/
    └── codebase/
        ├── ARCHITECTURE.md              # This file: system design, layers, data flow
        ├── STRUCTURE.md                 # This file: directory layout, naming conventions
        └── (STACK.md, INTEGRATIONS.md, CONVENTIONS.md, TESTING.md, CONCERNS.md → on request)
```

## Directory Purposes

**crates/lumen-core/:**
- Purpose: Shared primitives (IDs, hashing, crypto, errors, time)
- Contains: `AgentId`, `Blake3Hash`, `Ed25519` wrappers, `Rng` trait, `Error` enum
- Key files: `ids.rs` (newtype IDs), `crypto.rs` (signing/verification wrappers), `hash.rs` (BLAKE3), `error.rs` (Result type)

**crates/lumen-fixed/:**
- Purpose: Deterministic fixed-point arithmetic (Q-format) for ZK-bound code
- Contains: `Q16_16`, `Q8_24` types, quantization utilities
- Key files: `q16_16.rs`, `q8_24.rs`, `quant.rs` (i8 quantizer)
- Constraint: `calibration` feature disabled in proof-bound binaries (f32 only offline)

**crates/lumen-capability/:**
- Purpose: Signed capability tokens and policy enforcement
- Contains: `Capability` (signed token), `PolicyEngine` (verify + replay defense), `Action` (enum of gated actions), `Resource` (target of action)
- Key files: `capability.rs` (token structure), `policy.rs` (enforcement), `audit.rs` (deterministic logging)

**crates/lumen-channel/:**
- Purpose: Typed async message transport between isolation boundaries
- Contains: `SecureChannel` trait (send/recv bytes or typed messages), implementations: `InProcChannel`, `AttestedChannel`, `EncryptedChannel` (feature-gated), `TeeChannel` (stub)
- Key files: `lib.rs` (trait), `inproc.rs`, `attested.rs`, `encrypted.rs` (crypto-channel feature), `tee.rs` (TEE stub)

**crates/lumen-defense/:**
- Purpose: Real-time prompt-injection / jailbreak detection (sub-µs/kB)
- Contains: `DefenseEngine` (three-stage pipeline), `Verdict` enum, jailbreak lexicon (~30 triggers)
- Key files: `engine.rs` (pipeline), `lexicon.rs` (Aho-Corasick corpus), `regex_stage.rs` (RegexSet patterns), `heuristics.rs` (scoring)

**crates/lumen-provenance/:**
- Purpose: Model file provenance (hash verification, signature, SBOM)
- Contains: `ModelManifest`, `verify_model()` function, format sniffers (ONNX, Safetensors, GGUF), SBOM generation
- Key files: `manifest.rs` (manifest struct), `verify.rs` (hash/sig check), `sbom.rs` (CycloneDX emission)

**crates/lumen-zkml/:**
- Purpose: Pluggable ZK proof system abstraction
- Contains: `ProvingSystem` trait (generic over witness/public/proof/vk types), `MockCommitmentProver` (BLAKE3), `Verification` enum
- Key files: `mock.rs` (BLAKE3 commitment implementation), `verification.rs` (Verification::CommitmentOnly vs ZkVerified)

**crates/lumen-inference/:**
- Purpose: Backend-agnostic completion generation with streaming support
- Contains: `InferenceEngine` trait, concrete engines: `DummyEngine`, `CandleEngine` (ONNX, feature: candle), `LlamaCppEngine` (stub, feature: llama-cpp), `ChannelEngine` (TEE forward)
- Key files: `backend.rs` (trait + factory), `dummy.rs` (deterministic test engine), `streaming.rs` (StreamingEngine trait), `loader.rs` (model verification + loading)

**crates/lumen-sandbox/:**
- Purpose: WASM isolation via wasmtime with deterministic config and capability-gated host imports
- Contains: `Sandbox` (module runner), `SandboxConfig` (deterministic engine setup), `HostState` (policy, agent, audit), host imports (`lumen_log`, `lumen_call_tool`)
- Key files: `runner.rs` (run_module execution), `config.rs` (deterministic_engine), `imports.rs` (host FFI implementations), `host.rs` (HostState)

**crates/lumen-agent/:**
- Purpose: Orchestrates agent step pipeline (defense → infer → policy → tool → prove)
- Contains: `AgentRuntime` (main orchestrator), `StepResult` (output), `Tool` / `ToolRegistry` (executable tools), routing decision types
- Key files: `runtime.rs` (AgentRuntime::step pipeline), `tool.rs` (Tool trait + ToolRegistry), `route.rs` (RoutingDecision types)

**crates/lumen-orchestrator/:**
- Purpose: Multi-agent supervisor with capability-gated inter-agent messaging
- Contains: `Orchestrator` (registry + inboxes), `AgentHandle`, `InterAgentMessage`, `MessagePolicy`
- Key files: `supervisor.rs` (all logic)

**crates/lumen-sdk/:**
- Purpose: WASM-targeted SDK for agent code (no_std compatible)
- Contains: `log()`, `call_tool()` host import wrappers, `LogLevel` enum, `ToolError` enum
- Key files: `lib.rs` (all public exports; 100% of SDK)
- Constraint: `no_std`, compiled to `wasm32-unknown-unknown`

**crates/lumen-sdk-macros/:**
- Purpose: Proc-macro for `#[lumen_agent]` attribute
- Contains: Macro expansion for generating `_start()` WASM entry point
- Key files: `lib.rs`

**crates/lumen-attestation/:**
- Purpose: Parse Intel TDX Quote v4 and AMD SEV-SNP Attestation Report v2 (format-only verification in v0.3)
- Contains: `AttestationDoc` enum, `TdxQuote`, `SevSnpReport`, wire format parsers
- Key files: `tdx.rs`, `sev_snp.rs`, `lib.rs` (parse dispatcher)

**crates/lumen-onchain/:**
- Purpose: Emit and deploy on-chain verifiers (EVM Solidity + Mina o1js) for routing proof verification
- Contains: `Chain` enum, `VerifierMeta`, `emit_artifacts()`, `deploy_evm()` / `deploy_mina()`
- Key files: `emit.rs` (code generation), `deploy.rs` (deployment automation), `evm.rs` (Solidity), `mina.rs` (o1js)

**crates/lumen-cli/:**
- Purpose: User-facing command-line interface
- Contains: Seven subcommands (init, verify-model, sbom, defend, prove, run, verifier)
- Key files: `main.rs` (entry + dispatch), `cmd/*.rs` (subcommand implementations), `policy_file.rs` (TOML loading)

**agents/:** (Excluded from workspace)
- Purpose: Example WASM agents demonstrating SDK usage
- Contains: `echo-agent/` (minimal agent calling host tools)
- Key files: Agent `src/lib.rs`, `Cargo.toml` with `target: wasm32-unknown-unknown`

## Naming Conventions

**Files:**
- **Trait definitions:** `lib.rs` (e.g., `SecureChannel` trait in `lumen-channel/src/lib.rs`)
- **Concrete implementations:** `{name}.rs` matching implementation (e.g., `inproc.rs`, `encrypted.rs` in lumen-channel)
- **Type definitions:** `{plural_of_type}.rs` (e.g., `ids.rs` for `AgentId`, `CapabilityId`, etc.)
- **Entry points:** `main.rs` (CLI binary), `lib.rs` (library crates)
- **Tests:** `{module}_test.rs` or `tests/{feature}_test.rs`
- **Examples:** `examples/{demo_name}.rs`
- **Config:** `config.rs` when config struct is main export

**Directories:**
- Crate name prefix: `lumen-{feature}` (snake_case, hyphenated)
- Module pattern: One feature = one crate (e.g., `lumen-defense`, `lumen-zkml`)
- Nested modules: Within a crate, use `.rs` files not subdirs (flat structure preferred)

**Functions:**
- **Constructors:** `new()`, `with_*()` for builder variants (e.g., `PolicyEngine::new()`, `HostState::with_now()`)
- **Factory functions:** `create_*()` or `{type_name}_*()` (e.g., `create_engine()`, `build_stream()`)
- **Verifiers:** `verify_*()`, `check()`, `is_*()` for booleans (e.g., `verify_model()`, `is_human_readable()`)
- **Type conversions:** `as_*()`, `to_*()`, `from_*()` following Rust conventions
- **Async functions:** `_async()` suffix only if no `.await` syntax available; prefer async/await

**Types:**
- **Newtype IDs:** `{Entity}Id` (e.g., `AgentId`, `ToolId`, `CapabilityId`)
- **Enums:** `PascalCase` with `camelCase` variants (e.g., `Verdict::Block`, `Verification::CommitmentOnly`)
- **Traits:** `{Action}Engine` or `{Concept}` (e.g., `InferenceEngine`, `SecureChannel`, `ProvingSystem`)
- **Structs:** `PascalCase` (e.g., `ModelManifest`, `ToolCall`, `SamplingParams`)
- **Constants:** `SCREAMING_SNAKE_CASE` (e.g., `MAX_NONCES`, `DEFAULT_INBOX_DEPTH`)

**Modules:**
- **Public modules:** Exported in `lib.rs` (e.g., `pub mod policy;`, `pub mod capability;`)
- **Private modules:** File-only or nested (not re-exported)

## Key File Locations

**Entry Points:**
- `crates/lumen-cli/src/main.rs` — CLI binary entry
- `crates/lumen-agent/src/runtime.rs:99` — `AgentRuntime::step()` API entry
- `crates/lumen-sdk/src/lib.rs` — WASM SDK exports (`log`, `call_tool`)
- `agents/echo-agent/src/lib.rs` — Example WASM agent

**Configuration:**
- `Cargo.toml` (workspace root) — Workspace members, lints, workspace dependencies
- `crates/lumen-sandbox/src/config.rs` — Deterministic WASM engine configuration
- `crates/lumen-inference/src/backend.rs` — Backend selection + sampling parameter defaults
- `crates/lumen-cli/src/policy_file.rs` — Policy TOML schema + loading

**Core Logic:**
- `crates/lumen-core/src/` — All primitives (IDs, hashing, crypto, errors)
- `crates/lumen-agent/src/runtime.rs` — Main step pipeline
- `crates/lumen-capability/src/policy.rs` — Capability verification
- `crates/lumen-sandbox/src/runner.rs` — WASM execution
- `crates/lumen-defense/src/engine.rs` — Defense pipeline
- `crates/lumen-inference/src/backend.rs` — Engine trait + factories
- `crates/lumen-zkml/src/mock.rs` — Mock BLAKE3 prover

**Testing:**
- `crates/lumen-agent/tests/determinism.rs` — Determinism validation
- `crates/lumen-sandbox/tests/wasm_echo_agent.rs` — WASM sandbox tests
- `crates/lumen-capability/tests/policy_enforcement.rs` — Policy check tests

## Where to Add New Code

**New inference backend:**
1. Create `crates/lumen-inference/src/{backend_name}.rs`
2. Implement `InferenceEngine` trait
3. Add variant to `BackendConfig` enum in `backend.rs`
4. Extend `create_engine()` factory function
5. Add feature flag in `lumen-inference/Cargo.toml`

**New capability resource type:**
1. Add variant to `Resource` enum in `crates/lumen-capability/src/resource.rs`
2. Implement `matches_action()` for new `Action` variant
3. Add matching rule to `PolicyEngine::check()` in `policy.rs`
4. Update audit logging in `audit.rs`

**New defense stage:**
1. Add function to `crates/lumen-defense/src/{stage_name}.rs`
2. Call from `DefenseEngine::analyze()` pipeline in `engine.rs`
3. Return `Verdict` or score contribution
4. Register corpus version in `DefenseEngine::corpus_version()`

**New tool type:**
1. Create handler struct implementing `Tool` trait in `crates/lumen-agent/src/tool.rs`
2. Register in `ToolRegistry` via `register()` method
3. Include required `Capability` with matching `Resource::CallTool(tool_id)`

**New on-chain verifier:**
1. Add variant to `Chain` enum in `crates/lumen-onchain/src/lib.rs`
2. Create `{chain}.rs` module with codegen function
3. Update `emit::emit_artifacts()` to call new codegen
4. Update `deploy::deploy()` to handle new chain

**New WASM host import:**
1. Add FFI signature in `crates/lumen-sdk/src/lib.rs` (behind `#[cfg(target_arch = "wasm32")]`)
2. Implement panic in non-WASM in `lib.rs`
3. Add host-side handler in `crates/lumen-sandbox/src/imports.rs`
4. Register import in `crates/lumen-sandbox/src/runner.rs` linker

**New CLI subcommand:**
1. Create `crates/lumen-cli/src/cmd/{command}.rs`
2. Define `Args` struct with clap derive macros
3. Implement `run()` function
4. Add variant to `Cmd` enum in `main.rs`
5. Dispatch in `main()` match block

**New model format support:**
1. Create format sniff function in `crates/lumen-provenance/src/{format}.rs`
2. Add `Format` variant in `manifest.rs`
3. Call sniff from `verify_model()` in `verify.rs`

## Special Directories

**Generated / Non-committed:**
- `target/` — Cargo build output (in .gitignore)
- `.git/` — Git repository (not part of codebase)

**Build artifacts (in git):**
- `Cargo.lock` — Locked dependency versions (committed for reproducibility)

**Documentation:**
- `README.md` — Project overview
- `CLAUDE.md` — Project instructions (Korean, checked in)
- `kernel-comp.md` — Air-gap dependency audit (checked in)
- `.planning/codebase/` — GSD codebase maps (generated, committed)

**CI/CD:**
- `.github/workflows/` — GitHub Actions (linter, tests, clippy, fmt)

**Examples:**
- `agents/echo-agent/` — Sample WASM agent (excluded from workspace to avoid build issues)

---

*Structure analysis: 2026-05-07*
