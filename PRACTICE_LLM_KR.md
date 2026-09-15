# 로컬 LLM 실습 가이드

[![Language](https://img.shields.io/badge/PRACTICE_LLM-English_Ver-blue?style=for-the-badge)](PRACTICE_LLM.md)

이 문서는 **llama.cpp `llama-server` 백엔드** 로 Lumen 위에서 실제 LLM 을 돌리는 방법을 단계별로 설명합니다. 보안 모델 (핀된 엔진 바이너리, 검증된 모델, `/props` 바인딩), `lumen run` 을 통한 정책 파일 경로, 프로그램 경로, 스트리밍, 도구 호출, 샘플링, 양자화, 자주 발생하는 오류를 다룹니다. 설계 근거는 [INFERENCE_KR.md](INFERENCE_KR.md) 에 있습니다.

---

## 목차

1. [사전 준비](#1-사전-준비)
2. [엔진 바이너리 준비](#2-엔진-바이너리-준비)
3. [모델 파일과 매니페스트 준비](#3-모델-파일과-매니페스트-준비)
4. [경로 A: 정책 파일과 `lumen run`](#4-경로-a-정책-파일과-lumen-run)
5. [경로 B: 프로그램에서 Spawn 또는 Attach](#5-경로-b-프로그램에서-spawn-또는-attach)
6. [생성: 스트리밍](#6-생성-스트리밍)
7. [AgentRuntime 연동](#7-agentruntime-연동)
8. [도구 호출과 문법 제약](#8-도구-호출과-문법-제약)
9. [SamplingParams 조정](#9-samplingparams-조정)
10. [양자화 수준 선택](#10-양자화-수준-선택)
11. [Ed25519 서명 검증 (프로덕션)](#11-ed25519-서명-검증-프로덕션)
12. [자주 발생하는 오류](#12-자주-발생하는-오류)

---

## 1. 사전 준비

### Cargo feature

백엔드는 순수 Rust 이며 C 툴체인이 필요 없지만 opt-in 입니다.

```toml
[dependencies]
lumen-inference = { path = "crates/lumen-inference", features = ["llama-server"] }
lumen-agent     = { path = "crates/lumen-agent" }
lumen-provenance = { path = "crates/lumen-provenance" }
lumen-core      = { path = "crates/lumen-core" }
tokio   = { version = "1", features = ["rt-multi-thread", "macros"] }
futures = "0.3"
```

`lumen-cli` 는 이 feature 를 기본으로 켜므로 `cargo build -p lumen-cli` 에 이미 포함됩니다.

```bash
$ cargo build -p lumen-inference --features llama-server
```

### `llama-server` 바이너리

인터넷이 되는 머신에서 타겟 하드웨어 (CPU, CUDA, Metal, Vulkan) 에 맞게 llama.cpp 를 빌드하고, 만들어진 `llama-server` 실행 파일을 폐쇄망으로 반입합니다.

```bash
$ git clone https://github.com/ggml-org/llama.cpp
$ cmake -B build -DGGML_CUDA=ON      # 또는 -DGGML_METAL=ON, CPU 는 옵션 없음
$ cmake --build build --config Release -t llama-server
$ ls build/bin/llama-server
```

최근 릴리즈면 어느 것이든 동작합니다. Lumen 은 `/health`, `/props`, `/completion` 과 `LLAMA_ARG_API_KEY` 환경변수만 사용합니다.

---

## 2. 엔진 바이너리 준비

Lumen 은 BLAKE3 해시가 핀과 다른 엔진은 기동을 거부하므로, 신뢰된 머신에서 한 번 해시합니다.

```bash
$ b3sum build/bin/llama-server
# 3f2a...  build/bin/llama-server
```

Lumen 프리미티브로도 가능합니다.

```rust
use lumen_core::Blake3Hash;
let hash = Blake3Hash::of_file(std::path::Path::new("/opt/llama.cpp/llama-server"))?;
println!("{hash}");
```

바이너리는 전용 사용자 소유의 읽기 전용으로 두세요. 해시는 정책 파일의 `binary_hash` 또는 `SpawnSpec::binary_hash` 에 들어갑니다.

---

## 3. 모델 파일과 매니페스트 준비

### 3-1. GGUF 모델 다운로드

| 모델 | 양자화 | 파일 크기 | 권장 RAM |
|---|---|---|---|
| Qwen2.5-1.5B-Instruct-Q8_0.gguf | Q8_0 | ~1.7 GB | 4 GB |
| Phi-3-mini-4k-instruct-q4.gguf | Q4_K_M | ~2.2 GB | 4 GB |
| Mistral-7B-Instruct-v0.2-Q4_K_M.gguf | Q4_K_M | ~4.1 GB | 8 GB |
| Llama-3.1-8B-Instruct-Q4_K_M.gguf | Q4_K_M | ~4.9 GB | 10 GB |

별도 토크나이저 파일은 필요 없습니다. GGUF 에 내장되어 있고 `llama-server` 가 그것을 사용합니다.

### 3-2. 해시와 매니페스트

```bash
$ b3sum models/qwen2.5-1.5b-q8.gguf
```

```toml
# model.toml
name    = "qwen2.5-1.5b"
version = "1.0"
path    = "models/qwen2.5-1.5b-q8.gguf"
format  = "Gguf"
hash    = "a1b2c3..."   # 64자 hex
```

```bash
$ cargo run -p lumen-cli -- verify-model --manifest model.toml --file models/qwen2.5-1.5b-q8.gguf
```

---

## 4. 경로 A: 정책 파일과 `lumen run`

가장 단순한 production 경로는 핀된 정책 파일에 모든 것을 선언하는 것입니다. `lumen init` 이 템플릿을 출력하며, 관련 부분은 다음과 같습니다.

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
model       = "qwen2.5-1.5b"     # 위 [[models]] 항목을 가리킴
n_ctx       = "4096"
gpu_layers  = "99"
parallel    = "1"
```

step 하나 실행:

```bash
$ cargo run -p lumen-cli -- run \
    --policy policy.toml \
    --policy-hash "$(b3sum policy.toml | cut -d' ' -f1)" \
    --prompt 'Return {"tool":"add","args":{"a":2,"b":3}}'
```

순서대로 일어나는 일:

1. 정책 파일의 BLAKE3 를 `--policy-hash` 와 대조합니다.
2. 모든 `[[models]]` 매니페스트를 검증합니다.
3. 엔진 바이너리 해시를 검사하고, 새 API 키로 Unix 소켓에 `llama-server` 를 띄우고, `/health` 를 폴링하고, `/props.model_path` 를 검증된 모델 경로와 비교합니다.
4. 등록 도구 (`echo`, `add`) 로부터 GBNF 문법을 생성해 모든 completion 에 붙입니다.
5. step 실행: defense, 추론, 정책 검사, 도구 실행, 라우팅 commitment.

이미 실행 중인 서버 (예: TEE 안 systemd 가 관리) 에 붙으려면:

```toml
[inference.params]
mode         = "attach"
endpoint     = "unix:/run/lumen/llama.sock"
api_key_file = "/run/lumen/llama.key"   # 0600
model        = "qwen2.5-1.5b"           # 여전히 /props.model_path 로 바인드
```

---

## 5. 경로 B: 프로그램에서 Spawn 또는 Attach

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
    // 1. 검증된 모델 핸들 (개발용 hash-only 로더).
    let manifest: ModelManifest = toml::from_str(&std::fs::read_to_string("model.toml")?)?;
    let handle = VerifiedModelLoader::hash_only().load(manifest.path.as_path(), &manifest)?;

    // 2. 핀된 엔진 바이너리.
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

    // 3. 기동, 준비 대기, 서빙 모델 바인드.
    let engine = LlamaServerEngine::from_config(LlamaServerConfig::spawn(spec)).await?;
    println!("{:?}", engine.info());

    // 4. Completion.
    let params = SamplingParams { max_tokens: 128, ..Default::default() };
    let out = engine.complete("Rust 언어의 장점 세 가지를 설명해줘.", &params).await?;
    println!("{}", out.text);

    engine.shutdown().await?; // drop 시에도 종료됨
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

`expected_model` 이 없으면 엔진은 경고를 남깁니다. 서빙 중인 가중치가 어떤 검증 핸들에도 바인드되지 않기 때문입니다. 개발 중에만 사용하세요.

### 5-3. 레지스트리 경유

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

## 6. 생성: 스트리밍

```rust
use futures::StreamExt;
use lumen_inference::{StreamingEngine, SamplingParams};

let params = SamplingParams { max_tokens: 512, temperature: 0.8, top_p: 0.95, seed: None, ..Default::default() };
let mut stream = engine.stream_complete("한국 전통 음식을 설명해줘.", &params).await?;

while let Some(item) = stream.next().await {
    let token = item?;
    print!("{}", token.text);
    if let Some(reason) = token.finish_reason {
        eprintln!("\n[finished: {reason:?}]");
    }
}
```

스트림을 drop 하면 소켓이 닫히고 `llama-server` 가 생성을 중단합니다. 토큰 사이 유휴 타임아웃은 `LlamaServerConfig::request_timeout` (기본 300초) 입니다.

---

## 7. AgentRuntime 연동

```rust
use lumen_agent::AgentRuntime;
use lumen_inference::Engines;

let engine = Arc::new(engine);
let runtime = AgentRuntime::builder(agent_id, policy)
    .engines(Engines::from_dual(engine))   // InferenceEngine + StreamingEngine 을 함께 wiring
    .tool(echo_tool)?
    .capability(echo_tool_id, echo_cap)
    .proving_vk(vk)
    .policy_hash(policy_hash)
    .sampling(SamplingParams { max_tokens: 256, ..Default::default() })
    .build()?;

let result = runtime.step("...").await?;
println!("{:?}", result.completion.tool_call);
```

`stream_step` 은 토큰마다 `StreamEvent::Token` 을 내고 `StreamEvent::Complete` 로 끝납니다. 누적 텍스트는 마지막에 도구 호출로 파싱되므로 스트리밍·비스트리밍 step 의 라우팅이 동일합니다.

---

## 8. 도구 호출과 문법 제약

Lumen 의 도구 호출 규약은 JSON 객체 하나입니다.

```json
{"tool": "add", "args": {"a": 2, "b": 3}}
```

프롬프트에서 모델에게 이 규약을 알려주고 (채팅 템플릿은 에이전트 책임), 등록되지 않은 도구가 절대 생성되지 않도록 문법으로 출력을 제약합니다.

```rust
use lumen_core::ToolId;
use lumen_inference::ToolCallGrammar;

let grammar = ToolCallGrammar::new([ToolId::new("echo")?, ToolId::new("add")?]).gbnf();
let cfg = LlamaServerConfig::spawn(spec).grammar(grammar);
```

문법이 설정되면 모든 completion 이 도구 호출 형태로 강제됩니다. 같은 엔진이 자유 형식 질문에도 답해야 하면 같은 서버에 엔진 두 개 (또는 attach 설정 두 개) 를 두세요. 하나는 라우팅용 문법 있음, 하나는 문법 없음.

---

## 9. SamplingParams 조정

| 파라미터 | 기본값 | llama-server 필드 | 비고 |
|---|---|---|---|
| `max_tokens` | 256 | `n_predict` | 상한 |
| `temperature` | 0.0 | `temperature` | 0.0 = greedy |
| `top_p` | 1.0 | `top_p` | 1.0 = 비활성 |
| `top_k` | 0 | `top_k` | 0 = 비활성 |
| `repetition_penalty` | 1.0 | `repeat_penalty` | 1.1 ~ 1.3 이면 반복 억제 |
| `seed` | `Some(0)` | `seed` | `None` 이면 서버가 선택 |
| `stop_sequences` | 비어 있음 | `stop` | 종료 이유 `StopSequence` |

결정론적 라우팅 (기본): `temperature: 0.0`, `seed: Some(n)`, `parallel = 1`, `cache_prompt = false`. 창의적 생성: `temperature`/`top_p` 를 올리고 `seed: None`.

---

## 10. 양자화 수준 선택

| GGUF 레벨 | 7B 크기 | 품질 | 용도 |
|---|---|---|---|
| Q4_K_M | ~4.1 GB | 양호 | CPU 와 소형 GPU 의 기본값 |
| Q5_K_M | ~4.8 GB | 더 좋음 | 균형 |
| Q8_0 | ~7.2 GB | fp16 근접 | 정확도가 중요하고 메모리가 허용될 때 |
| F16 | ~13.5 GB | 기준 | 메모리가 넉넉한 GPU |

`lumen-inference` 의 `QuantizationConfig` 는 감사용으로 의도한 레벨을 기록할 뿐이며, 실제로 `llama-server` 가 무엇을 돌리는지는 GGUF 파일이 결정합니다. `FixedPoint` 종류는 ZK 라우팅 경로 전용이고 LLM 가중치와 무관합니다.

---

## 11. Ed25519 서명 검증 (프로덕션)

고보안 환경에서는 **신뢰된 서명자의 Ed25519 서명이 매니페스트에 있는 모델** 만 허용하세요.

### 11-1. 매니페스트 서명 (배포자 측)

```rust
use lumen_core::{OsRng, SigningKey};
use lumen_provenance::ModelManifest;

let signing_key = SigningKey::generate(&mut OsRng);   // 비밀키는 HSM 또는 Vault 에 보관
let mut manifest: ModelManifest = toml::from_str(&std::fs::read_to_string("model.toml")?)?;
manifest.sign_with(&signing_key)?;
std::fs::write("model_signed.toml", toml::to_string(&manifest)?)?;
```

### 11-2. 서명 검증 로더 (사용자 측)

```rust
use lumen_core::VerifyingKey;
use lumen_inference::VerifiedModelLoader;

let trusted_key: VerifyingKey = /* 사전 배포 */;
let loader = VerifiedModelLoader::new(vec![trusted_key]);
let manifest: ModelManifest = toml::from_str(&std::fs::read_to_string("model_signed.toml")?)?;
let handle = loader.load(manifest.path.as_path(), &manifest)?;   // 해시 + 서명 모두 필수
```

엔진 바이너리는 현재 해시 핀만 됩니다. [INFERENCE_KR.md](INFERENCE_KR.md#10-알려진-공백과-로드맵) 의 로드맵을 참고하세요.

---

## 12. 자주 발생하는 오류

| 오류 | 원인 | 조치 |
|---|---|---|
| `provenance: engine binary hash mismatch` | 바이너리가 바뀌었거나 핀이 틀림 | 신뢰된 바이너리를 다시 해시하고 `binary_hash` 갱신 |
| `provenance: hash mismatch for <model>` | 모델 파일이 매니페스트와 다름 | 다시 받거나 `hash` 수정 |
| `provenance: llama-server serves ... but verified model is ...` | 서버가 검증 핸들과 다른 파일을 로드함 | `model` 이 서버가 쓰는 파일을 가리키게 수정 |
| `inference: llama-server exited during startup: exit status: N` | 잘못된 바이너리, 지원되지 않는 GPU, 너무 큰 모델 | 같은 인자로 바이너리를 직접 실행해 stderr 확인 |
| `inference: llama-server at unix:... not ready within 120s` | 모델 로드가 `startup_timeout_secs` 보다 느림 | 타임아웃을 올리거나 `gpu_layers` 를 줄임 |
| `inference: llama-server: http 401 (api key rejected)` | attach 모드에서 키가 틀리거나 없음 | `api_key_file` 수정 |
| `invalid: endpoint: plaintext tcp to non-loopback ... refused` | 원격 TCP 엔드포인트 | 엔진을 로컬에서 돌리거나 `SecureChannel` peer 뒤에 배치 |
| `invalid: backend \`llama-server\`: unknown params [...]` | `[inference.params]` 오타 | INFERENCE_KR.md 의 키 표와 대조 |
