# Local LLM Practice Guide

[![Language](https://img.shields.io/badge/PRACTICE_LLM-Korean_Ver-blue?style=for-the-badge)](PRACTICE_LLM_KR.md)

This document walks through running a real LLM under Lumen with the **llama.cpp `llama-server` backend**. It covers the security model (pinned engine binary, verified model, bound `/props`), the policy-file route through `lumen run`, the programmatic route, streaming, tool calls, sampling, quantization and common errors. The design rationale lives in [INFERENCE.md](INFERENCE.md).

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Preparing the Engine Binary](#2-preparing-the-engine-binary)
3. [Preparing the Model File and Manifest](#3-preparing-the-model-file-and-manifest)
4. [Route A: Policy File and `lumen run`](#4-route-a-policy-file-and-lumen-run)
5. [Route B: Programmatic Spawn or Attach](#5-route-b-programmatic-spawn-or-attach)
6. [Generation: Streaming](#6-generation-streaming)
7. [AgentRuntime Integration](#7-agentruntime-integration)
8. [Tool Calls and Grammar Constraints](#8-tool-calls-and-grammar-constraints)
9. [Tuning SamplingParams](#9-tuning-samplingparams)
10. [Choosing a Quantization Level](#10-choosing-a-quantization-level)
11. [Ed25519 Signature Verification (Production)](#11-ed25519-signature-verification-production)
12. [Common Errors](#12-common-errors)

---

## 1. Prerequisites

### Cargo feature

The backend is pure Rust and needs no C toolchain, but it is opt-in:

```toml
[dependencies]
lumen-inference = { path = "crates/lumen-inference", features = ["llama-server"] }
lumen-agent     = { path = "crates/lumen-agent" }
lumen-provenance = { path = "crates/lumen-provenance" }
lumen-core      = { path = "crates/lumen-core" }
tokio   = { version = "1", features = ["rt-multi-thread", "macros"] }
futures = "0.3"
```

`lumen-cli` enables the feature by default, so `cargo build -p lumen-cli` already includes it.

```bash
$ cargo build -p lumen-inference --features llama-server
```

### A `llama-server` binary

Build llama.cpp on a connected machine for the target hardware (CPU, CUDA, Metal, Vulkan) and carry the resulting `llama-server` executable into the air-gapped site:

```bash
$ git clone https://github.com/ggml-org/llama.cpp
$ cmake -B build -DGGML_CUDA=ON      # or -DGGML_METAL=ON, or nothing for CPU
$ cmake --build build --config Release -t llama-server
$ ls build/bin/llama-server
```

Any recent release works; Lumen relies only on `/health`, `/props`, `/completion` and the `LLAMA_API_KEY` environment variable.

The verified combination is llama.cpp b10603 (macOS arm64, Metal) with Qwen3.8-27B UD-Q6_K_XL. Both spawn and attach modes, non-streaming `/completion` and SSE streaming, the GBNF tool-call grammar, and API key rejection (401) were exercised through `lumen run` and the library path.

---

## 2. Preparing the Engine Binary

Lumen refuses to start an engine whose BLAKE3 hash does not match the pin, so hash it once on a trusted machine:

```bash
$ b3sum build/bin/llama-server
# 3f2a...  build/bin/llama-server
```

Or with Lumen's own primitive:

```rust
use lumen_core::Blake3Hash;
let hash = Blake3Hash::of_file(std::path::Path::new("/opt/llama.cpp/llama-server"))?;
println!("{hash}");
```

Keep the binary read-only and owned by a dedicated user. The hash goes into the policy file (`binary_hash`) or into `SpawnSpec::binary_hash`.

---

## 3. Preparing the Model File and Manifest

### 3-1. Download a GGUF model

| Model | Quantization | File size | Recommended RAM |
|---|---|---|---|
| Qwen2.5-1.5B-Instruct-Q8_0.gguf | Q8_0 | ~1.7 GB | 4 GB |
| Phi-3-mini-4k-instruct-q4.gguf | Q4_K_M | ~2.2 GB | 4 GB |
| Mistral-7B-Instruct-v0.2-Q4_K_M.gguf | Q4_K_M | ~4.1 GB | 8 GB |
| Llama-3.1-8B-Instruct-Q4_K_M.gguf | Q4_K_M | ~4.9 GB | 10 GB |

No separate tokenizer file is needed; GGUF embeds it and `llama-server` uses it.

### 3-2. Hash and manifest

```bash
$ b3sum models/qwen2.5-1.5b-q8.gguf
```

```toml
# model.toml
name    = "qwen2.5-1.5b"
version = "1.0"
path    = "models/qwen2.5-1.5b-q8.gguf"
format  = "Gguf"
hash    = "a1b2c3..."   # 64 hex chars
```

```bash
$ cargo run -p lumen-cli -- verify-model --manifest model.toml --file models/qwen2.5-1.5b-q8.gguf
```

---

## 4. Route A: Policy File and `lumen run`

The simplest production path is to declare everything in the pinned policy file. `lumen init` prints a template; the relevant part:

```toml
[[models]]
name    = "qwen2.5-1.5b"
version = "1.0"
path    = "models/qwen2.5-1.5b-q8.gguf"
format  = "Gguf"
hash    = "a1b2c3..."

[inference]
backend = "llama-server"

[inference.params]
mode        = "spawn"
endpoint    = "unix:/run/lumen/llama.sock"
binary      = "/opt/llama.cpp/llama-server"
binary_hash = "3f2a..."
model       = "qwen2.5-1.5b"     # refers to the [[models]] entry above
n_ctx       = "4096"
gpu_layers  = "99"
parallel    = "1"
```

Run one step:

```bash
$ cargo run -p lumen-cli -- run \
    --policy policy.toml \
    --policy-hash "$(b3sum policy.toml | cut -d' ' -f1)" \
    --prompt 'Return {"tool":"add","args":{"a":2,"b":3}}'
```

What happens in order:

1. The policy file's BLAKE3 is checked against `--policy-hash`.
2. Every `[[models]]` manifest is verified.
3. The engine binary hash is checked, `llama-server` is spawned with a fresh API key on the Unix socket, `/health` is polled, and `/props.model_path` is compared with the verified model path.
4. A GBNF grammar is generated from the registered tools (`echo`, `add`) and attached to every completion.
5. The step runs: defense, inference, policy check, tool execution, routing commitment.

To attach to a server that is already running (for example one managed by systemd inside the TEE):

```toml
[inference.params]
mode         = "attach"
endpoint     = "unix:/run/lumen/llama.sock"
api_key_file = "/run/lumen/llama.key"   # 0600
model        = "qwen2.5-1.5b"           # still bound via /props.model_path
```

---

## 5. Route B: Programmatic Spawn or Attach

### 5-1. Spawn

```rust
use std::path::Path;
use std::sync::Arc;

use lumen_core::Blake3Hash;
use lumen_inference::llama::{Endpoint, LlamaServerConfig, LlamaServerEngine, SpawnSpec};
use lumen_inference::{InferenceEngine, SamplingParams, VerifiedModelLoader};
use lumen_provenance::ModelManifest;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Verified model handle (hash-only loader for development).
    let manifest: ModelManifest = toml::from_str(&std::fs::read_to_string("model.toml")?)?;
    let handle = VerifiedModelLoader::hash_only().load(manifest.path.as_path(), &manifest)?;

    // 2. Pinned engine binary.
    let binary_hash: Blake3Hash = "3f2a...".parse()?;
    let mut spec = SpawnSpec::new(
        "/opt/llama.cpp/llama-server",
        binary_hash,
        handle,
        Endpoint::Unix("/run/lumen/llama.sock".into()),
    );
    spec.n_ctx = 4096;
    spec.n_gpu_layers = Some(99);
    spec.parallel = 1;

    // 3. Spawn, wait for readiness, bind the served model.
    let engine = LlamaServerEngine::from_config(LlamaServerConfig::spawn(spec)).await?;
    println!("{:?}", engine.info());

    // 4. Complete.
    let params = SamplingParams { max_tokens: 128, ..Default::default() };
    let out = engine.complete("Explain three advantages of Rust.", &params).await?;
    println!("{}", out.text);

    engine.shutdown().await?; // also happens on drop
    Ok(())
}
```

### 5-2. Attach

```rust
let engine = LlamaServerEngine::from_config(
    LlamaServerConfig::attach(Endpoint::parse("unix:/run/lumen/llama.sock")?)
        .api_key(std::fs::read_to_string("/run/lumen/llama.key")?.trim())
        .expected_model(handle),
)
.await?;
```

Without `expected_model` the engine logs a warning: the served weights are then not bound to any verified handle. Use that only in development.

### 5-3. Through the registry

```rust
use std::collections::BTreeMap;
use lumen_inference::BackendRegistry;

let mut params = BTreeMap::new();
params.insert("mode".into(), "attach".into());
params.insert("endpoint".into(), "unix:/run/lumen/llama.sock".into());
params.insert("model".into(), "models/qwen2.5-1.5b-q8.gguf".into());
params.insert("model_hash".into(), "a1b2c3...".into());

let engines = BackendRegistry::with_builtins().build("llama-server", &params).await?;
```

---

## 6. Generation: Streaming

```rust
use futures::StreamExt;
use lumen_inference::{StreamingEngine, SamplingParams};

let params = SamplingParams { max_tokens: 512, temperature: 0.8, top_p: 0.95, seed: None, ..Default::default() };
let mut stream = engine.stream_complete("Describe traditional Korean cuisine.", &params).await?;

while let Some(item) = stream.next().await {
    let token = item?;
    print!("{}", token.text);
    if let Some(reason) = token.finish_reason {
        eprintln!("\n[finished: {reason:?}]");
    }
}
```

Dropping the stream closes the socket and `llama-server` aborts the generation. The idle timeout between tokens is `LlamaServerConfig::request_timeout` (default 300 s).

---

## 7. AgentRuntime Integration

```rust
use lumen_agent::AgentRuntime;
use lumen_inference::Engines;

let engine = Arc::new(engine);
let runtime = AgentRuntime::builder(agent_id, policy)
    .engines(Engines::from_dual(engine))   // wires InferenceEngine + StreamingEngine
    .tool(echo_tool)?
    .capability(echo_tool_id, echo_cap)
    .proving_vk(vk)
    .policy_hash(policy_hash)
    .sampling(SamplingParams { max_tokens: 256, ..Default::default() })
    .build()?;

let result = runtime.step("...").await?;
println!("{:?}", result.completion.tool_call);
```

`stream_step` emits `StreamEvent::Token` per token and finishes with `StreamEvent::Complete`. The accumulated text is parsed for a tool call at the end, so streaming and non-streaming steps route identically.

---

## 8. Tool Calls and Grammar Constraints

Lumen's tool-call convention is a single JSON object:

```json
{"tool": "add", "args": {"a": 2, "b": 3}}
```

Tell the model about it in the prompt (chat templates are the agent's responsibility), and constrain the output with a grammar so an unregistered tool can never be generated:

```rust
use lumen_core::ToolId;
use lumen_inference::ToolCallGrammar;

let grammar = ToolCallGrammar::new([ToolId::new("echo")?, ToolId::new("add")?]).gbnf();
let cfg = LlamaServerConfig::spawn(spec).grammar(grammar);
```

With a grammar set, every completion is forced into the tool-call shape. If the same engine must also answer free-form questions, run two engines (or two attach configs) against the same server: one with a grammar for routing, one without.

---

## 9. Tuning SamplingParams

| Parameter | Default | llama-server field | Notes |
|---|---|---|---|
| `max_tokens` | 256 | `n_predict` | hard cap |
| `temperature` | 0.0 | `temperature` | 0.0 = greedy |
| `top_p` | 1.0 | `top_p` | 1.0 = off |
| `top_k` | 0 | `top_k` | 0 = off |
| `repetition_penalty` | 1.0 | `repeat_penalty` | 1.1 to 1.3 suppresses loops |
| `seed` | `Some(0)` | `seed` | `None` lets the server pick |
| `stop_sequences` | empty | `stop` | finish reason `StopSequence` |

Deterministic routing (default): `temperature: 0.0`, `seed: Some(n)`, `parallel = 1`, `cache_prompt = false`. Creative generation: raise `temperature`/`top_p`, set `seed: None`.

---

## 10. Choosing a Quantization Level

| GGUF level | 7B size | Quality | Use |
|---|---|---|---|
| Q4_K_M | ~4.1 GB | good | default for CPU and small GPUs |
| Q5_K_M | ~4.8 GB | better | balanced |
| Q8_0 | ~7.2 GB | near fp16 | when accuracy matters and memory allows |
| F16 | ~13.5 GB | reference | GPU with ample memory |

`QuantizationConfig` in `lumen-inference` records the intended level for audit; the GGUF file itself determines what `llama-server` runs. The `FixedPoint` kind is reserved for the ZK routing path and is unrelated to LLM weights.

---

## 11. Ed25519 Signature Verification (Production)

In high-security environments allow only **models whose manifest carries an Ed25519 signature from a trusted signer**.

### 11-1. Signing the manifest (distributor side)

```rust
use lumen_core::{OsRng, SigningKey};
use lumen_provenance::ModelManifest;

let signing_key = SigningKey::generate(&mut OsRng);   // keep the private key in an HSM or vault
let mut manifest: ModelManifest = toml::from_str(&std::fs::read_to_string("model.toml")?)?;
manifest.sign_with(&signing_key)?;
std::fs::write("model_signed.toml", toml::to_string(&manifest)?)?;
```

### 11-2. Verifying loader (consumer side)

```rust
use lumen_core::VerifyingKey;
use lumen_inference::VerifiedModelLoader;

let trusted_key: VerifyingKey = /* pre-distributed */;
let loader = VerifiedModelLoader::new(vec![trusted_key]);
let manifest: ModelManifest = toml::from_str(&std::fs::read_to_string("model_signed.toml")?)?;
let handle = loader.load(manifest.path.as_path(), &manifest)?;   // hash + signature, both required
```

The engine binary is currently hash-pinned only; see the roadmap in [INFERENCE.md](INFERENCE.md#10-known-gaps-and-roadmap).

---

## 12. Common Errors

| Error | Cause | Fix |
|---|---|---|
| `provenance: engine binary hash mismatch` | binary changed or wrong pin | rehash the trusted binary, update `binary_hash` |
| `provenance: hash mismatch for <model>` | model file differs from manifest | re-download or fix `hash` |
| `provenance: llama-server serves ... but verified model is ...` | server loaded a different file than the verified handle | point `model` at the same file the server uses |
| `inference: llama-server exited during startup: exit status: N` | wrong binary, unsupported GPU, model too large | run the binary by hand with the same arguments and read stderr |
| `inference: llama-server at unix:... not ready within 120s` | model load slower than `startup_timeout_secs` | raise the timeout or reduce `gpu_layers` |
| `inference: llama-server: http 401 (api key rejected)` | attach mode with a wrong or missing key | fix `api_key_file` |
| `invalid: endpoint: plaintext tcp to non-loopback ... refused` | remote TCP endpoint | run the engine locally or behind a `SecureChannel` peer |
| `invalid: backend \`llama-server\`: unknown params [...]` | typo in `[inference.params]` | compare with the key table in INFERENCE.md |
