---
last_mapped_commit: 7c6fc8c
analysis_date: 2026-05-07
focus: architecture
---

# Architecture

**Analysis Date:** 2026-05-07

## System Overview

Lumen is a zero-trust, verifiable AI-agent framework that isolates agent logic in WASM sandboxes while enforcing security through capability-based access control, cryptographic provenance, and optional zero-knowledge proofs of tool routing decisions.

```text
┌────────────────────────────────────────────────────────────────────┐
│                         Agent Runtime                              │
│  `crates/lumen-agent/src/runtime.rs`                              │
│  - step() pipeline: defense → infer → policy → tool → prove       │
└────────────┬────────────────────────────────┬─────────────────────┘
             │                                │
    ┌────────▼──────────┐          ┌──────────▼─────────────┐
    │  Defense Engine   │          │ Inference Engine      │
    │ `lumen-defense/`  │          │ `lumen-inference/`    │
    │ - Aho-Corasick    │          │ - DummyEngine         │
    │ - RegexSet        │          │ - CandleEngine (ONNX) │
    │ - Heuristics      │          │ - LlamaCppEngine      │
    └───────────────────┘          │ - ChannelEngine (TEE) │
                                   └──────────┬────────────┘
                                              │
         ┌────────────────────────────────────┼─────────────────────────┐
         │                                    │                         │
    ┌────▼──────────────┐   ┌─────────────────▼────┐   ┌───────────────▼──┐
    │ Policy Engine     │   │ Tool Registry        │   │ Proving System   │
    │ `lumen-capability`│   │ `lumen-agent/tool.rs`│   │ `lumen-zkml/`    │
    │ - Verify Cap      │   │ - Tool handlers      │   │ - MockProver     │
    │ - Replay defense  │   │ - JSON args          │   │ - ezkl stub      │
    │ - Audit logging   │   │                      │   │ - BLAKE3 commit  │
    └───────────────────┘   └──────────────────────┘   └──────────────────┘
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| **AgentRuntime** | Orchestrates defense→infer→policy→tool→prove pipeline | `crates/lumen-agent/src/runtime.rs` |
| **DefenseEngine** | Real-time prompt injection / jailbreak detection | `crates/lumen-defense/src/engine.rs` |
| **InferenceEngine** | Backend-agnostic completion generation (trait) | `crates/lumen-inference/src/backend.rs` |
| **PolicyEngine** | Capability verification, replay prevention, audit logging | `crates/lumen-capability/src/policy.rs` |
| **Sandbox** | WASM isolation via wasmtime with deterministic config | `crates/lumen-sandbox/src/runner.rs` |
| **ProvingSystem** | Abstract proof generation (mock BLAKE3 commitment) | `crates/lumen-zkml/src/mock.rs` |
| **SecureChannel** | Typed async message transport (inproc / encrypted / attested) | `crates/lumen-channel/src/lib.rs` |
| **ModelManifest** | Provenance: BLAKE3 + Ed25519 signature verification | `crates/lumen-provenance/src/manifest.rs` |
| **Orchestrator** | Multi-agent supervisor with capability-gated inter-agent messaging | `crates/lumen-orchestrator/src/supervisor.rs` |

## Pattern Overview

**Overall:** Multi-crate workspace with **strict layering** via capability-based security and WASM sandboxing. Core pattern:

1. **Entry point** → Agent step invoked by host (CLI, SDK)
2. **Defense filter** → Prompt analyzed for jailbreak patterns
3. **Inference** → LLM or reasoning engine produces completion + optional tool call
4. **Policy gate** → Capability checked before tool execution
5. **Tool exec** → WASM agent calls host tool via explicit capability
6. **ZK bind** → Routing decision committed to BLAKE3 (mock) or ZK proof
7. **Audit trail** → All decisions logged with deterministic field names

**Security boundary:** Host (LLM inference, TEE trust) ↔ WASM guest (agent logic) = **capability-only**.

## Layers

**Core Primitives:**
- Purpose: Lightweight, zero-IO types used by all crates
- Location: `crates/lumen-core/src/`
- Contains: `AgentId`, `CapabilityId`, `Blake3Hash`, `Ed25519` wrappers, `Timestamp`, error types
- Depends on: `elib-k0-nt/*` crypto (BLAKE3, ED25519, X25519, AES, RNG)
- Used by: All other crates

**Fixed-Point Arithmetic:**
- Purpose: Deterministic quantization (no floating-point nondeterminism for ZK)
- Location: `crates/lumen-fixed/src/`
- Contains: `Q16_16`, `Q8_24` fixed-point types, quantization utilities
- Depends on: `lumen-core`
- Used by: `lumen-inference` (weight quantization), `lumen-zkml` (deterministic arithmetic)

**Cryptography & Capabilities:**
- Purpose: Signed capability tokens, policy enforcement, audit
- Location: `crates/lumen-capability/src/`
- Contains: `Capability` (signed token), `PolicyEngine` (verify + replay defense), `Resource` (file / net / tool / inference)
- Depends on: `lumen-core`
- Used by: `lumen-sandbox`, `lumen-orchestrator`, `lumen-agent`

**Secure Channels:**
- Purpose: Typed message transport between sandbox and host (or inter-agent)
- Location: `crates/lumen-channel/src/`
- Contains: `SecureChannel` trait, `InProcChannel`, `AttestedChannel`, `EncryptedChannel` (feature-gated), `TeeChannel` (stub)
- Depends on: `lumen-core`
- Used by: `lumen-sandbox`, `lumen-orchestrator`, `lumen-inference::ChannelEngine`

**Defense Engine:**
- Purpose: Sub-µs prompt-injection / jailbreak detection
- Location: `crates/lumen-defense/src/`
- Contains: `DefenseEngine` (Aho-Corasick lexicon + RegexSet + heuristics), `Verdict`
- Depends on: `lumen-core`, `aho-corasick`, `regex`
- Used by: `lumen-agent::runtime`

**Model Provenance:**
- Purpose: BLAKE3 hash verification, signature validation, SBOM emission
- Location: `crates/lumen-provenance/src/`
- Contains: `ModelManifest`, `verify_model()`, ONNX/Safetensors/GGUF sniffing
- Depends on: `lumen-core`, `safetensors`
- Used by: `lumen-inference::loader`, CLI commands

**Proving System:**
- Purpose: Pluggable ZK backend abstraction (mock BLAKE3, ezkl stub)
- Location: `crates/lumen-zkml/src/`
- Contains: `ProvingSystem` trait, `MockCommitmentProver` (BLAKE3), `Verification` enum
- Depends on: `lumen-core`
- Used by: `lumen-agent::runtime`

**Inference Engine:**
- Purpose: Backend-agnostic completion generation with streaming support
- Location: `crates/lumen-inference/src/`
- Contains: `InferenceEngine` trait, `DummyEngine` (deterministic), `CandleEngine` (ONNX), `ChannelEngine` (TEE forward)
- Depends on: `lumen-core`, `lumen-provenance`, `lumen-fixed`
- Optional deps: `candle-*` (feature: `candle`)
- Used by: `lumen-agent::runtime`

**WASM Sandbox:**
- Purpose: Capability-gated wasmtime isolation for agent code
- Location: `crates/lumen-sandbox/src/`
- Contains: `Sandbox`, `SandboxConfig::deterministic_engine()`, `HostState`, capability-gated imports (`lumen_log`, `lumen_call_tool`)
- Depends on: `lumen-core`, `lumen-capability`, `wasmtime`
- Used by: Agent runtime for untrusted WASM execution

**Agent Runtime:**
- Purpose: Orchestrates full step pipeline (defense → infer → policy → tool → prove)
- Location: `crates/lumen-agent/src/`
- Contains: `AgentRuntime`, `AgentRuntimeBuilder`, `StepResult`, `Tool` / `ToolRegistry`, routing decision types
- Depends on: `lumen-core`, `lumen-defense`, `lumen-inference`, `lumen-capability`, `lumen-zkml`
- Used by: `lumen-orchestrator`, CLI commands, SDK agents

**Orchestrator (Multi-agent):**
- Purpose: Supervisor for isolated multi-agent systems with capability-gated inter-agent messaging
- Location: `crates/lumen-orchestrator/src/`
- Contains: `Orchestrator`, `AgentHandle`, `AgentInbox` (bounded mpsc), `InterAgentMessage`
- Depends on: `lumen-core`, `lumen-agent`, `lumen-capability`, `lumen-channel`
- Used by: Multi-agent deployments

**SDK & Macros:**
- Purpose: WASM-targeted SDK for agent code (`no_std` compatible, `#[lumen_agent]` proc-macro)
- Location: `crates/lumen-sdk/src/`, `crates/lumen-sdk-macros/src/`
- Contains: `log()`, `call_tool()` host imports, `LogLevel`, `ToolError`
- Depends on: None (SDK is `no_std`)
- Used by: WASM agents compiled to `wasm32-unknown-unknown`

**Attestation (TEE):**
- Purpose: Parse and format-verify Intel TDX and AMD SEV-SNP attestation documents
- Location: `crates/lumen-attestation/src/`
- Contains: `AttestationDoc`, `TdxQuote`, `SevSnpReport`, format parsing (no crypto verification yet)
- Depends on: `lumen-core`
- Used by: TEE channel backends (future), host trust decisions

**On-Chain Verifiers:**
- Purpose: Emit contract scaffolds (EVM Solidity + Mina o1js) for on-chain proof verification
- Location: `crates/lumen-onchain/src/`
- Contains: `emit_artifacts()`, `deploy_evm()`, routing verifier contracts
- Depends on: `lumen-core`
- Used by: CLI verifier commands

**CLI:**
- Purpose: User-facing command-line interface (init, verify-model, sbom, defend, prove, run, verifier)
- Location: `crates/lumen-cli/src/`
- Contains: Subcommands, policy file loading, end-to-end agent execution
- Depends on: All policy/inference/zkml/provenance crates
- Used by: `lumen` binary

## Data Flow

### Primary Request Path (Agent Step)

1. **Input:** Host calls `AgentRuntime::step(prompt: &str)` (`crates/lumen-agent/src/runtime.rs:99`)
2. **Defense:** `DefenseEngine::analyze(prompt)` → `Verdict` (block or pass) (`crates/lumen-defense/src/engine.rs`)
3. **Inference:** `InferenceEngine::complete(prompt, params)` → `Completion { text, tool_call }` (`crates/lumen-inference/src/backend.rs`)
4. **Policy Check:** For each `tool_call`, `PolicyEngine::check(cap, agent, Action::CallTool(tool_id), now)` verifies capability (`crates/lumen-capability/src/policy.rs:61`)
5. **Tool Execution:** If policy passes, invoke tool via `ToolHandler::call(args_json)` → JSON result (`crates/lumen-agent/src/tool.rs`)
6. **ZK Bind:** `RoutingDecision` (public inputs + witness) created (`crates/lumen-agent/src/route.rs`), passed to `ProvingSystem::prove()` → `MockProof` (`crates/lumen-zkml/src/mock.rs`)
7. **Verification:** `ProvingSystem::verify(vk, public, proof)` → `Verification::CommitmentOnly` or `Invalid` (mock never `ZkVerified`) (`crates/lumen-zkml/src/verification.rs`)
8. **Output:** `StepResult { completion, tool_output, routing, proof, verification, defense_verdict }` (`crates/lumen-agent/src/runtime.rs:22`)

### Streaming Path

1. **Input:** Host calls `AgentRuntime::stream_step(prompt: &str)` 
2. **Defense:** Same as step path
3. **Stream Init:** `StreamingEngine::stream_complete(prompt, params)` → `TokenStream` yields `Token` events
4. **Tee Stream:** Accumulate tokens to full `Completion`, then proceed as step path
5. **Event Output:** Stream yields `StreamEvent::Token(token)` in real-time, finally `StreamEvent::Complete(StepResult)`

### WASM Sandbox Execution

1. **Guest:** WASM agent calls `lumen_call_tool(tool_ptr, tool_len, args_ptr, args_len)` (imported from host)
2. **Host Memcpy:** `lumen_sandbox::imports::lumen_call_tool()` copies `tool_id` and `args_json` out of guest linear memory (`crates/lumen-sandbox/src/imports.rs`)
3. **Policy Gate:** `HostState::policy.check(cap, agent, Action::CallTool(tool_id), now)` blocks if capability missing
4. **Tool Lookup:** `ToolRegistry::call(tool_id, args_json)` executes registered handler
5. **Result Buffer:** Result (or error code) placed in shared memory; guest reads via future `lumen_recv` import (v0.4)

### Multi-Agent Inter-Communication

1. **Send:** Agent A calls `Orchestrator::send_agent_message(recipient, msg)` 
2. **Policy Check:** `PolicyEngine::check(cap, agent, Action::SendAgentMessage(recipient_id), now)`
3. **Routing:** Message queued in `AgentInbox` (bounded mpsc) of agent B
4. **Receive:** Agent B polls `AgentInbox::recv()` → typed message via `SecureChannel::recv<T>()`

**State Management:**
- Each agent in orchestrator owns **separate tokio task** with no shared memory access
- `Orchestrator` maintains agent registry (`HashMap<AgentId, AgentHandle>`) and inbox map
- All inter-agent data passed through `SecureChannel` (postcard-encoded)
- No global mutable state (enforced by Rust ownership model)

## Key Abstractions

**InferenceEngine:**
- Purpose: Pluggable completion backend selection
- Examples: `DummyEngine` (deterministic test), `CandleEngine` (ONNX routing), `ChannelEngine` (TEE forward)
- Pattern: Trait object with builder factory (`backend::create_engine()`) selects concrete type at runtime
- File: `crates/lumen-inference/src/backend.rs`

**ProvingSystem:**
- Purpose: Pluggable ZK proof backend (mock BLAKE3 or ezkl)
- Pattern: Generic trait with associated types (`Witness`, `PublicInputs`, `Proof`, `Vk`)
- Enables boxed trait objects in hot path without backend knowledge
- File: `crates/lumen-zkml/src/lib.rs:48`

**SecureChannel:**
- Purpose: Abstract typed message transport (inproc, encrypted, attested, TEE)
- Pattern: Trait with `send_bytes()` / `recv_bytes()`, plus convenience `send<T: Serialize>()` / `recv<T: Deserialize>()`
- Implementations hidden behind feature flags (e.g., `crypto-channel` for encrypted)
- File: `crates/lumen-channel/src/lib.rs:38`

**BackendConfig:**
- Purpose: Runtime selection of inference backend + sampling parameters
- Pattern: Serializable enum with per-backend config fields
- Used by CLI to instantiate correct `InferenceEngine` type
- File: `crates/lumen-inference/src/backend.rs`

**Capability:**
- Purpose: Unforgeable signed token binding audience, resource, nonce, expiry, and issuer signature
- Pattern: Serializable struct with Ed25519 signature; `PolicyEngine::check()` validates before action
- Design: No implicit permissions — every meaningful action requires explicit capability
- File: `crates/lumen-capability/src/capability.rs`

**RoutingDecision:**
- Purpose: Record of tool selection with public inputs (prompt hash, policy hash, tool ID) and witness (args hash, defense corpus)
- Pattern: Opaque witness never leaves proving system in true ZK; mock prover exposes it for testing
- Enables on-chain re-verification of tool routing logic
- File: `crates/lumen-agent/src/route.rs:39`

## Entry Points

**CLI Binary:**
- Location: `crates/lumen-cli/src/main.rs`
- Triggers: `lumen init|verify-model|sbom|defend|prove|run|verifier`
- Responsibilities: Policy loading, model verification, defense testing, ZK proof demo, full agent step execution, contract generation

**SDK Agent (WASM):**
- Location: `crates/lumen-sdk/src/lib.rs`
- Triggers: Agent code compiled to `wasm32-unknown-unknown`, linked against SDK
- Responsibilities: Expose `log()` and `call_tool()` wrappers that marshal arguments to host imports
- Pattern: Optional `#[lumen_agent]` proc-macro generates `_start()` entry point

**Agent Runtime API (Library):**
- Location: `crates/lumen-agent/src/runtime.rs:99`
- Triggers: `AgentRuntime::step(prompt)` or `AgentRuntime::stream_step(prompt)`
- Responsibilities: Execute full defense→infer→policy→tool→prove pipeline, return `StepResult`

## Architectural Constraints

- **Threading:** Tokio async runtime; single-threaded or multi-threaded depending on `tokio` feature setup. Agent tasks isolated per tokio task, no shared memory.
- **Global state:** Orchestrator maintains shared `Arc<PolicyEngine>`, `Arc<InferenceEngine>`, agent registry. No module-level singletons beyond these.
- **Circular imports:** None detected. Dependency graph is DAG: `lumen-core` ← all others; `lumen-inference` ← `lumen-agent`; `lumen-zkml` ← `lumen-agent`.
- **Determinism:** WASM engine config disables SIMD, threads, relaxed ops, forces Cranelift NaN canonicalization. Fixed-point arithmetic enforced for ZK-bound code. RNG seed pinned in tests.
- **Memory isolation:** WASM linear memory separate from host; capability-gated imports prevent host data leak.
- **Namespace collision:** Feature flags (`candle`, `llama-cpp`, `crypto-channel`, `ezkl`) ensure only enabled code is compiled.

## Anti-Patterns

### Implicit Permissions

**What happens:** Code assumes a tool can be called without checking `PolicyEngine` first.

**Why it's wrong:** Enables privilege escalation in multi-agent deployments. A malicious tool could execute without capability checks, bypassing audit.

**Do this instead:** Always wrap tool invocation in `policy.check(cap, agent, Action::CallTool(tool_id), now)` before calling `ToolHandler::call()`. See `crates/lumen-agent/src/runtime.rs:111` for the correct pattern.

### Floating-Point in Proof-Bound Code

**What happens:** LLM weight quantization or routing scores computed with `f32` operations.

**Why it's wrong:** Floating-point is nondeterministic across CPU architectures (denormals, FMA contraction, x87 extended precision). ZK proof fails reproducibility.

**Do this instead:** Use `lumen-fixed::Q16_16` or `Q8_24` for all computation in proof paths. Calibration (f32↔Qn_m conversion) permitted only offline, behind the `calibration` feature flag disabled in ZK-bound binaries. See `crates/lumen-fixed/src/lib.rs`.

### Direct WASM Host Function Calls

**What happens:** Agent WASM code calls `lumen_log` or `lumen_call_tool` directly instead of via SDK wrapper.

**Why it's wrong:** Raw FFI pointers bypass SDK safety checks (linear memory bounds validation, panic handling).

**Do this instead:** Use `lumen_sdk::log(level, msg)` and `lumen_sdk::call_tool(tool_id, args_json)`. These handle pointer/length marshalling safely. See `crates/lumen-sdk/src/lib.rs:81`.

### Unbounded Nonce Cache

**What happens:** `PolicyEngine::seen_nonces` allowed to grow without limit, consuming unbounded memory under reuse attacks.

**Why it's wrong:** Attacker repeatedly issues capabilities with unique nonces, exhausting host memory.

**Do this instead:** Cache enforces `MAX_NONCES = 65_536` limit with LRU eviction. See `crates/lumen-capability/src/policy.rs:35`.

## Error Handling

**Strategy:** Result-based with `lumen_core::Error` enum. No panic-on-error; all failable operations return `Result<T>`.

**Patterns:**
- **Defense block:** `Error::Defense(reason)` stops execution immediately, returns blocked verdict to caller
- **Policy denial:** `Error::Capability(reason)` logged to audit trail, blocks action
- **Inference failure:** `Error::Inference(reason)` returned; step fails (no tool call attempted)
- **Proof failure:** `Error::Proving(reason)` returned; step result includes `Verification::Invalid`
- **Provenance mismatch:** `Error::Provenance(reason)` halts model load; invalid model cannot reach inference engine

## Cross-Cutting Concerns

**Logging:** Via `tracing` crate with `tracing-subscriber` in CLI. All audit events use deterministic field names for external SIEM ingestion. See `crates/lumen-capability/src/audit.rs`.

**Validation:** 
- **Capabilities:** `PolicyEngine::check()` validates signature, replay, audience, expiry, resource matching
- **Models:** `ModelManifest::verify_model()` checks BLAKE3 hash and Ed25519 signature before load
- **Attestation:** `AttestationDoc::parse()` validates wire format (v0.4 will add crypto verification)

**Authentication:** 
- **Agent identity:** `AgentId` (16-byte random) identifies sender; no human username involved
- **Capability issuer:** `VerifyingKey` (Ed25519 public) identifies issuer; must be in `PolicyEngine::trusted` list
- **Policy enforcement:** Per-action capability requirement; no role-based access control (purely capability-based)

---

*Architecture analysis: 2026-05-07*
