# Local LLM Practice Guide

[![Language](https://img.shields.io/badge/PRACTICE_LLM-Korean_Ver-blue?style=for-the-badge)](PRACTICE_LLM.md)

This document walks through how to use Lumen's LLM inference pipeline step by step. It covers the security model (enforced verification), quantization choices, streaming generation, and agent runtime integration.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Preparing the Model File](#2-preparing-the-model-file)
3. [Computing the BLAKE3 Hash and Writing the Manifest](#3-computing-the-blake3-hash-and-writing-the-manifest)
4. [Generation: Text](#4-generation-text)
5. [Generation: Streaming](#5-generation-streaming)
6. [AgentRuntime Integration](#6-agentruntime-integration)
7. [Tuning SamplingParams](#7-tuning-samplingparams)
8. [Choosing a Quantization Level](#8-choosing-a-quantization-level)
9. [Ed25519 Signature Verification (Production)](#9-ed25519-signature-verification-production)
10. [Performance and Memory Guide](#10-performance-and-memory-guide)
11. [Common Errors](#11-common-errors)

---

## 1. Prerequisites

### Add the feature to Cargo.toml

```toml
[dependencies]
lumen-inference = { path = "crates/lumen-inference", features = ["candle-llm"] }
lumen-agent     = { path = "crates/lumen-agent" }
lumen-provenance = { path = "crates/lumen-provenance" }
lumen-core      = { path = "crates/lumen-core" }
tokio   = { version = "1", features = ["rt-multi-thread", "macros"] }
futures = "0.3"
```

> **candle-llm vs candle**
> - `candle`: Enables only the existing ONNX routing engine (`CandleEngine`).
> - `candle-llm`: Enables GGUF quantized LLM + HuggingFace tokenizer + streaming generation. Choose this when you need actual text generation.

Verify the build with:

```bash
$ cargo build -p lumen-inference --features candle-llm
```

---

## 2. Preparing the Model File

### 2-1. Download a GGUF model

The `candle-llm` backend supports **GGUF format** quantized models. Download a `.gguf` file directly from [HuggingFace](https://huggingface.co/).

Recommended starter models (by size):

| Model                                  | Quantization | File Size | Recommended RAM |
|--------------------------------------|--------|---------|--------|
| Qwen2.5-1.5B-Instruct-Q8_0.gguf      | Q8_0   | ~1.7 GB | 4 GB   |
| Phi-3-mini-4k-instruct-q4.gguf       | Q4_K_M | ~2.2 GB | 4 GB   |
| Mistral-7B-Instruct-v0.2-Q4_K_M.gguf | Q4_K_M | ~4.1 GB | 8 GB   |
| Llama-3.1-8B-Instruct-Q4_K_M.gguf    | Q4_K_M | ~4.9 GB | 10 GB  |

> **Air-gapped environments**: In environments without internet access, download the files on an external network first, compute the hash, and write the manifest manually.

### 2-2. Download tokenizer.json

Download `tokenizer.json` from the **Files** tab on the HuggingFace model card page. The model and tokenizer must be from the same version.

```
models/
  mistral-7b-q4.gguf   <- GGUF weights
  tokenizer.json        <- HuggingFace tokenizer
```

---

## 3. Computing the BLAKE3 Hash and Writing the Manifest

Lumen enforces BLAKE3 hash verification before loading any model. You must compute the hash of the model file first.

### 3-1. Compute the hash via CLI

```bash
# Install b3sum (cargo install b3sum)
$ b3sum models/mistral-7b-q4.gguf
# Output: a1b2c3d4... *models/mistral-7b-q4.gguf
```

Alternatively, compute it with a small Rust snippet:

```rust
use lumen_core::Blake3Hash;
use std::path::Path;

fn main() -> std::io::Result<()> {
    let hash = Blake3Hash::of_file(Path::new("models/mistral-7b-q4.gguf"))?;
    println!("{}", hash); // 64-char lowercase hex
    Ok(())
}
```

### 3-2. Write the manifest TOML

Write `model.toml` using the hash you computed:

```toml
# model.toml
name    = "Mistral-7B-Instruct-v0.2"
version = "0.2"
path    = "models/mistral-7b-q4.gguf"
format  = "Gguf"
hash    = "a1b2c3d4e5f6..." # 64-char hex from above
```

Supported `format` values are `"Safetensors"` | `"Onnx"` | `"Gguf"`.

### 3-3. Verify with the lumen CLI

```bash
$ cargo run -p lumen-cli -- verify-model \
    --manifest model.toml \
    --file models/mistral-7b-q4.gguf
# Output: model verified: Mistral-7B-Instruct-v0.2 ...
```

---

## 4. Generation: Text

### 4-1. Hash-only loader (development)

```rust
use std::path::Path;
use std::sync::Arc;

use lumen_inference::{
    loader::VerifiedModelLoader,
    CandleLlmEngine,
    InferenceEngine,
    SamplingParams,
};
use lumen_provenance::{Format, ModelManifest};
use lumen_core::Blake3Hash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load the model file hash (pre-computed value)
    let expected_hash: Blake3Hash =
        "a1b2c3d4e5f6...".parse()?; // 64-char hex

    // 2. Create a hash-only verified loader (no signature check)
    let loader = VerifiedModelLoader::hash_only();

    // 3. Build the manifest and load
    let manifest = ModelManifest {
        name: "Mistral-7B-Instruct-v0.2".into(),
        version: "0.2".into(),
        path: "models/mistral-7b-q4.gguf".into(),
        format: Format::Gguf,
        hash: expected_hash,
        license: Some("Apache-2.0".into()),
        signature: None,
        signer: None,
    };
    let handle = loader.load(Path::new("models/mistral-7b-q4.gguf"), &manifest)?;

    // 4. Create the engine — compile error if VerifiedModelHandle is missing
    let engine = CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")?;

    // 5. Run inference
    let params = SamplingParams {
        max_tokens: 256,
        temperature: 0.7,
        top_p: 0.9,
        ..Default::default()
    };
    let completion = engine.complete("Explain three advantages of the Rust language.", &params).await?;
    println!("{}", completion.text);

    Ok(())
}
```

### 4-2. Loading from a manifest TOML file

Managing the manifest as a TOML file is also supported:

```rust
use lumen_provenance::ModelManifest;

let toml_str = std::fs::read_to_string("model.toml")?;
let manifest: ModelManifest = toml::from_str(&toml_str)?;

let handle = loader.load(manifest.path.as_path(), &manifest)?;
```

---

## 5. Generation: Streaming

`StreamingEngine::stream_complete` delivers tokens as they are generated, making it suitable for terminal output, WebSocket, SSE, and similar use cases.

### 5-1. Direct streaming

```rust
use futures::StreamExt;
use lumen_inference::{streaming::StreamingEngine, SamplingParams};

let params = SamplingParams {
    max_tokens: 512,
    temperature: 0.8,
    top_p: 0.95,
    seed: None, // None produces different results on each run
    ..Default::default()
};

let mut stream = engine.stream_complete("Describe traditional Korean cuisine.", &params).await?;

while let Some(result) = stream.next().await {
    let token = result?;
    print!("{}", token.text);

    if let Some(reason) = token.finish_reason {
        println!();
        eprintln!("[finished: {:?}]", reason);
        break;
    }
}
```

### 5-2. Collecting all tokens

To drain the stream and assemble it into a single string:

```rust
use futures::StreamExt;

let mut text = String::new();
let mut stream = engine.stream_complete(prompt, &params).await?;

while let Some(tok) = stream.next().await {
    text.push_str(&tok?.text);
}
println!("{text}");
```

### 5-3. Early cancellation

Dropping the stream immediately stops generation. To receive only the first 100 tokens:

```rust
let mut stream = engine.stream_complete(prompt, &params).await?;
let mut count = 0;

while let Some(tok) = stream.next().await {
    print!("{}", tok?.text);
    count += 1;
    if count >= 100 {
        break; // drop here -> background task stops automatically
    }
}
```

---

## 6. AgentRuntime Integration

Connecting an LLM engine to the agent runtime produces the full pipeline: `defense` -> `infer` -> `policy` -> `tool` -> `ZK proof`, all operating together with LLM output.

### 6-1. Builder configuration

```rust
use std::sync::Arc;
use lumen_inference::{CandleLlmEngine, InferenceEngine, StreamingEngine};
use lumen_agent::AgentRuntime;

// Create the engine (see sections above)
let engine = Arc::new(CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")?);

let runtime = AgentRuntime::builder(agent_id, policy)
    // InferenceEngine: used in step()
    .inference(engine.clone() as Arc<dyn InferenceEngine>)
    // StreamingEngine: must be set to enable stream_step()
    .streaming_engine(engine as Arc<dyn StreamingEngine>)
    .tool(echo_tool)?
    .capability(echo_tool_id, echo_cap)
    .proving_vk(vk)
    .policy_hash(policy_hash)
    .sampling(SamplingParams {
        max_tokens: 512,
        temperature: 0.7,
        ..Default::default()
    })
    .build()?;
```

### 6-2. Regular step (returns after full response)

```rust
let result = runtime.step("echo hello world").await?;
println!("Response: {}", result.completion.text);
println!("Tool output: {:?}", result.tool_output);
println!("Verification: {:?}", result.verification);
```

### 6-3. Streaming step (real-time tokens + final result)

```rust
use futures::StreamExt;
use lumen_agent::StreamEvent;

let mut events = runtime.stream_step("Explain Rust's ownership model.").await?;

while let Some(event) = events.next().await {
    match event? {
        StreamEvent::Token(tok) => {
            // Print each token as it is generated
            print!("{}", tok.text);
        }
        StreamEvent::Complete(result) => {
            // Stream ended: access ZK proof, tool results, etc.
            println!("\n--- Generation complete ---");
            println!("Defense: {:?}", result.defense_verdict);
            println!("ZK verification: {:?}", result.verification);
            if let Some(output) = result.tool_output {
                println!("Tool output: {}", output);
            }
        }
    }
}
```

### 6-4. BackendConfig factory (concise configuration)

```rust
use lumen_inference::backend::{BackendConfig, create_engine};

let engine = create_engine(BackendConfig::CandleLlm {
    handle, // VerifiedModelHandle
    tokenizer_path: "models/tokenizer.json".into(),
})?;

// engine: Arc<dyn InferenceEngine>
let runtime = AgentRuntime::builder(agent_id, policy)
    .inference(engine)
    // For streaming, set StreamingEngine separately
    ...
    .build()?;
```

> [!TIP]
> `create_engine` returns `Arc<dyn InferenceEngine>`. If you need streaming,
> create the engine directly with `CandleLlmEngine::from_gguf` and pass it as both traits separately.

---

## 7. Tuning SamplingParams

| Parameter            | Default   | Description                                              |
|----------------------|-----------|----------------------------------------------------------|
| `max_tokens`         | 256       | Maximum number of tokens to generate                     |
| `temperature`        | 0.0       | 0.0 = greedy (deterministic); higher = more creative     |
| `top_p`              | 1.0       | Setting to 0.9 activates nucleus sampling                |
| `top_k`              | 0         | Retain only the top-k candidate tokens. 0 = disabled     |
| `repetition_penalty` | 1.0       | Values of `1.1`–`1.3` suppress repetition               |
| `seed`               | `Some(0)` | `None` produces random results on each run               |
| `stop_sequences`     | empty     | Stop generation when one of these strings appears        |

### Recommended settings by use case

```rust
// Deterministic: testing, ZK proof reproducibility
SamplingParams {
    temperature: 0.0,
    seed: Some(42),
    ..Default::default()
}

// Balanced: general conversation
SamplingParams {
    max_tokens: 512,
    temperature: 0.7,
    top_p: 0.9,
    seed: None,
    ..Default::default()
}

// Creative: writing, brainstorming
SamplingParams {
    max_tokens: 1024,
    temperature: 1.2,
    top_p: 0.95,
    repetition_penalty: 1.1,
    seed: None,
    ..Default::default()
}

// Code generation: low temperature, repetition suppression
SamplingParams {
    max_tokens: 512,
    temperature: 0.2,
    top_p: 0.95,
    stop_sequences: vec!["```".into()],
    ..Default::default()
}
```

> [!WARNING]
> **ZK reproducibility note**: When `temperature > 0`, output is non-deterministic.
> Bind only the **integer routing index** (tool-selection decision) to the ZK witness — do not bind the generated text itself.

---

## 8. Choosing a Quantization Level

`QuantizationConfig` is the configuration type that communicates quantization intent to the backend. Because the current `CandleLlmEngine` encodes quantization inside the GGUF file itself, **choosing the file determines the quantization level**.

```rust
use lumen_inference::quantize::{QuantizationConfig, GgufLevel};

// Q4: balance of speed and accuracy (most common)
let _q4 = QuantizationConfig::gguf_q4();  // GgufLevel::Q4K0

// Q8: higher accuracy, 2× memory
let _q8 = QuantizationConfig::gguf_q8();  // GgufLevel::Q8K0

// ZK path: tool routing scores in fixed-point
let _zk = QuantizationConfig::zk_fixed(); // FixedPointPrecision::Q16_16
```

### Comparison by GGUF level (7B model baseline)

| Level  | File Size | Min RAM | Speed    | Accuracy Loss |
|--------|---------|--------|-------|--------|
| Q4_K_M | ~4.1 GB | 6 GB   | Fast    | Low          |
| Q5_K_M | ~4.8 GB | 7 GB   | Medium  | Very low     |
| Q8_0   | ~7.2 GB | 10 GB  | Slow    | Minimal      |
| F16    | ~14 GB  | 18 GB  | Slowest | None         |

> [!NOTE]
> **All inference in the current implementation runs on CPU.** GPU/Metal support will be added as a feature flag in a future release.

---

## 9. Ed25519 Signature Verification (Production)

In high-security environments such as government or regulated deployments, allow only **models that carry an Ed25519 signature from a trusted signer**.

### 9-1. Signing the manifest (model distributor side)

```rust
use lumen_core::SigningKey;
use lumen_provenance::ModelManifest;

// Generate a signing key (store the private key in an HSM or Vault)
let signing_key = SigningKey::generate(&mut rand::thread_rng());
let verifying_key = signing_key.verifying_key();

// Sign the manifest
let mut manifest = ModelManifest {
    name: "Mistral-7B-Instruct-v0.2".into(),
    version: "0.2".into(),
    path: "models/mistral-7b-q4.gguf".into(),
    format: lumen_provenance::Format::Gguf,
    hash: expected_hash,
    license: Some("Apache-2.0".into()),
    signature: None,
    signer: None,
};
manifest.sign_with(&signing_key)?;

// Serialize to TOML and distribute
let toml_str = toml::to_string(&manifest)?;
std::fs::write("model_signed.toml", &toml_str)?;
```

### 9-2. Signature-verifying loader (model consumer side)

```rust
use lumen_core::VerifyingKey;
use lumen_inference::loader::VerifiedModelLoader;

// Trusted signer's public key (pre-distributed, hardcoded, or in a policy file)
let trusted_key: VerifyingKey = /* ... */;
let loader = VerifiedModelLoader::new(vec![trusted_key]);

let toml_str = std::fs::read_to_string("model_signed.toml")?;
let manifest: lumen_provenance::ModelManifest = toml::from_str(&toml_str)?;

// Verifies both BLAKE3 hash and Ed25519 signature — either failure returns an error
let handle = loader.load(manifest.path.as_path(), &manifest)?;
```

---

## 10. Performance and Memory Guide

### Why is the first inference slow?

`CandleLlmEngine::from_gguf` loads the entire GGUF file into RAM. This can take several seconds depending on model size. After loading, the weights are shared via `Arc<Mutex<ModelWeights>>`, so **there is no additional cost** for subsequent calls.

```rust
// Load once at process startup, reuse afterwards
static ENGINE: once_cell::sync::OnceCell<Arc<CandleLlmEngine>> = OnceCell::new();

let engine = ENGINE.get_or_try_init(|| {
    // This block executes only once
    CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")
        .map(Arc::new)
})?;
```

### Concurrent requests

When `AgentRuntime` is shared across multiple tasks, the model lock (`Mutex<ModelWeights>`) is held during generation. Concurrent LLM requests are processed serially. For high throughput, run multiple engine instances as separate tasks.

### Token generation speed targets (CPU baseline)

| Model Size | Quantization | Estimated Speed |
|-------|--------|-------------|
| 1B    | Q8_0   | 20–40 tok/s |
| 3B    | Q4_K_M | 10–20 tok/s |
| 7B    | Q4_K_M | 5–12 tok/s  |
| 7B    | Q8_0   | 3–7 tok/s   |

> [!NOTE]
> Results vary significantly depending on CPU core count, clock speed, and memory bandwidth.

---

## 11. Common Errors

### `Error::Provenance("hash mismatch")`

> The model file does not match the hash in the manifest.

- Confirm the download completed fully (recompute with `b3sum`).
- Verify the hex string in the manifest is exactly 64 characters.
- Confirm the file has not been modified.

### `Error::Inference("GGUF magic mismatch")`

> The file is not in GGUF format.

- Even a `.gguf` extension does not guarantee the correct format.
- Re-download or inspect the file header: `xxd models/model.gguf | head -1`.
- Expected output: `47475546` (`GGUF`).

### `Error::Inference("tokenizer load failed")`

> The `tokenizer.json` path is wrong or does not match the model.

- Confirm that `tokenizer.json` belongs to this specific model.
- Confirm the path is correct relative to the current working directory.

### `Error::NotImplemented("streaming engine not configured")`

> To use `stream_step()`, you must also call `streaming_engine()` on the builder.

```rust
let engine = Arc::new(CandleLlmEngine::from_gguf(&handle, tokenizer)?);
AgentRuntime::builder(...)
    .inference(engine.clone() as Arc<dyn InferenceEngine>)
    .streaming_engine(engine as Arc<dyn StreamingEngine>) // required
    ...
```

### Out of memory (OOM)

The model exceeds available RAM. Use a model file with a lower quantization level, or switch to a smaller model (1B–3B).

---

## Full Working Example

A minimal end-to-end example that integrates all the pieces above:

```rust
use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use lumen_agent::{AgentRuntime, StreamEvent};
use lumen_capability::PolicyEngine;
use lumen_core::{AgentId, Blake3Hash};
use lumen_inference::{
    loader::VerifiedModelLoader, CandleLlmEngine, InferenceEngine,
    SamplingParams, StreamingEngine,
};
use lumen_provenance::{Format, ModelManifest};
use lumen_zkml::mock::{keygen, MockVk};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Verify the model and create the engine
    let hash: Blake3Hash = "paste_b3sum_hex_here".parse()?;
    let manifest = ModelManifest {
        name: "my-llm".into(),
        version: "1.0".into(),
        path: "models/model.gguf".into(),
        format: Format::Gguf,
        hash,
        license: None,
        signature: None,
        signer: None,
    };
    let handle = VerifiedModelLoader::hash_only()
        .load(Path::new("models/model.gguf"), &manifest)?;

    let engine = Arc::new(
        CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")?
    );

    // 2. Configure the agent runtime
    let agent_id = AgentId::new("demo-agent").unwrap();
    let policy = Arc::new(PolicyEngine::default());
    let (vk, _sk) = keygen();
    let policy_hash = Blake3Hash::of(b"demo-policy-v1");

    let runtime = AgentRuntime::builder(agent_id, policy)
        .inference(engine.clone() as Arc<dyn InferenceEngine>)
        .streaming_engine(engine as Arc<dyn StreamingEngine>)
        .proving_vk(vk)
        .policy_hash(policy_hash)
        .sampling(SamplingParams {
            max_tokens: 256,
            temperature: 0.7,
            top_p: 0.9,
            ..Default::default()
        })
        .build()?;

    // 3. Run streaming inference
    println!("Generating...");
    let mut events = runtime.stream_step("Briefly explain the key features of Rust.").await?;

    while let Some(event) = events.next().await {
        match event? {
            StreamEvent::Token(tok) => print!("{}", tok.text),
            StreamEvent::Complete(result) => {
                println!("\n\n[ZK verification: {:?}]", result.verification);
            }
        }
    }

    Ok(())
}
```

---

## Next Steps

- [lumen-inference source](crates/lumen-inference/src/): Backend implementation code
- [INTRODUCTION.md](INTRODUCTION.md): Overall design philosophy and security model
- [llama-cpp feature](crates/lumen-inference/src/llama_cpp.rs): llama.cpp backend (to be completed in the next milestone)
- Run the full pipeline demo: `cargo run -p lumen-cli --example hello_agent`
