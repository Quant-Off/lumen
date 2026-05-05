# 로컬 LLM 실습 가이드

[![Language](https://img.shields.io/badge/PRACTICE_LLM-English_Ver-blue?style=for-the-badge)](PRACTICE_LLM_EN.md)

이 문서는 Lumen의 LLM 추론 파이프라인을 실제로 사용하는 방법을 단계별로 설명합니다. 보안 모델(검증 강제), 양자화 선택, 스트리밍 생성, 에이전트 런타임 연동을 다룹니다.

---

## 목차

1. [사전 준비](#1-사전-준비)
2. [모델 파일 준비](#2-모델-파일-준비)
3. [BLAKE3 해시 계산 및 매니페스트 작성](#3-blake3-해시-계산-및-매니페스트-작성)
4. [생성: 텍스트](#4-생성-텍스트)
5. [생성: 스트리밍](#5-생성-스트리밍)
6. [AgentRuntime 연동](#6-agentruntime-연동)
7. [SamplingParams 조정](#7-samplingparams-조정)
8. [양자화 수준 선택](#8-양자화-수준-선택)
9. [Ed25519 서명 검증 (프로덕션)](#9-ed25519-서명-검증-프로덕션)
10. [성능과 메모리 가이드](#10-성능과-메모리-가이드)
11. [자주 발생하는 오류](#11-자주-발생하는-오류)

---

## 1. 사전 준비

### Cargo.toml 에 feature 추가

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
> - `candle`: 기존 ONNX 라우팅 엔진(`CandleEngine`)만 활성화합니다.
> - `candle-llm`: GGUF 양자화 LLM + HuggingFace 토크나이저 + 스트리밍 생성을 활성화합니다. 실제 텍스트 생성이 필요하면 선택하세요.

다음 명령으로 빌드를 확인합니다.

```bash
$ cargo build -p lumen-inference --features candle-llm
```

---

## 2. 모델 파일 준비

### 2-1. GGUF 모델 다운로드

`candle-llm` 백엔드는 **GGUF 포맷** 양자화 모델을 지원합니다. [HuggingFace](https://huggingface.co/)에서 `.gguf` 파일을 직접 다운로드하세요.

권장 시작 모델(크기 순)은 다음과 같습니다.

| 모델                                   | 양자화    | 파일 크기   | 권장 RAM |
|--------------------------------------|--------|---------|--------|
| Qwen2.5-1.5B-Instruct-Q8_0.gguf      | Q8_0   | ~1.7 GB | 4 GB   |
| Phi-3-mini-4k-instruct-q4.gguf       | Q4_K_M | ~2.2 GB | 4 GB   |
| Mistral-7B-Instruct-v0.2-Q4_K_M.gguf | Q4_K_M | ~4.1 GB | 8 GB   |
| Llama-3.1-8B-Instruct-Q4_K_M.gguf    | Q4_K_M | ~4.9 GB | 10 GB  |

> **폐쇄망 환경**: 인터넷이 없는 환경에서는 외부망에서 미리 다운로드한 후 해시를 계산해 매니페스트를 작성하세요.

### 2-2. tokenizer.json 다운로드

HuggingFace 모델 카드 페이지의 **Files** 탭에서 `tokenizer.json` 을 다운로드합니다. 반드시 모델과 토크나이저가 같은 버전이어야 합니다.

```
models/
  mistral-7b-q4.gguf <- GGUF 가중치
  tokenizer.json     <- HuggingFace 토크나이저
```

---

## 3. BLAKE3 해시 계산 및 매니페스트 작성

Lumen은 모든 모델을 로드하기 전에 BLAKE3 해시를 강제 검증합니다. 먼저 모델 파일의 해시를 계산해야 합니다.

### 3-1. CLI로 해시 계산

```bash
# b3sum 설치 (cargo install b3sum)
$ b3sum models/mistral-7b-q4.gguf
# 출력: a1b2c3d4... *models/mistral-7b-q4.gguf
```

또는 소규모 Rust 스니펫으로 계산할 수 있습니다.

```rust
use lumen_core::Blake3Hash;
use std::path::Path;

fn main() -> std::io::Result<()> {
    let hash = Blake3Hash::of_file(Path::new("models/mistral-7b-q4.gguf"))?;
    println!("{}", hash); // 64자 소문자 hex
    Ok(())
}
```

### 3-2. 매니페스트 TOML 작성

계산한 해시로 `model.toml`을 작성합니다.

```toml
# model.toml
name    = "Mistral-7B-Instruct-v0.2"
version = "0.2"
path    = "models/mistral-7b-q4.gguf"
format  = "Gguf"
hash    = "a1b2c3d4e5f6..." # 위에서 계산한 64자 hex
```

지원 format 값은 `"Safetensors"` | `"Onnx"` | `"Gguf"`입니다.

### 3-3. lumen CLI로 검증 확인

```bash
$ cargo run -p lumen-cli -- verify-model \
    --manifest model.toml \
    --file models/mistral-7b-q4.gguf
# 출력: model verified: Mistral-7B-Instruct-v0.2 ...
```

---

## 4. 생성: 텍스트

### 4-1. 해시만 검증하는 로더 (개발 환경)

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
    // 1. 모델 파일 해시를 로드 (미리 계산된 값)
    let expected_hash: Blake3Hash =
        "a1b2c3d4e5f6...".parse()?; // 64자 hex

    // 2. 검증 로더 생성: 해시만 확인, 서명 없음
    let loader = VerifiedModelLoader::hash_only();

    // 3. 매니페스트 구성 및 로드
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

    // 4. 엔진 생성: VerifiedModelHandle 이 없으면 컴파일 에러
    let engine = CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")?;

    // 5. 추론
    let params = SamplingParams {
        max_tokens: 256,
        temperature: 0.7,
        top_p: 0.9,
        ..Default::default()
    };
    let completion = engine.complete("Rust 언어의 장점을 세 가지 설명해줘.", &params).await?;
    println!("{}", completion.text);

    Ok(())
}
```

### 4-2. 매니페스트 TOML 파일에서 로드

매니페스트를 TOML 파일로 관리하는 방식도 지원됩니다.

```rust
use lumen_provenance::ModelManifest;

let toml_str = std::fs::read_to_string("model.toml")?;
let manifest: ModelManifest = toml::from_str(&toml_str)?;

let handle = loader.load(manifest.path.as_path(), &manifest)?;
```

---

## 5. 생성: 스트리밍

`StreamingEngine::stream_complete`는 토큰이 생성되는 즉시 전달합니다. 터미널 출력, WebSocket, SSE 등에 적합합니다.

### 5-1. 직접 스트리밍

```rust
use futures::StreamExt;
use lumen_inference::{streaming::StreamingEngine, SamplingParams};

let params = SamplingParams {
    max_tokens: 512,
    temperature: 0.8,
    top_p: 0.95,
    seed: None, // None 이면 매 실행마다 다른 결과
    ..Default::default()
};

let mut stream = engine.stream_complete("한국의 전통 음식을 설명해줘.", &params).await?;

while let Some(result) = stream.next().await {
    let token = result?;
    print!("{}", token.text);

    if let Some(reason) = token.finish_reason {
        println!();
        eprintln!("[종료: {:?}]", reason);
        break;
    }
}
```

### 5-2. 전체 토큰 수집

스트림을 소진해 하나의 문자열로 조립할 때는 다음과 같이 할 수 있습니다.

```rust
use futures::StreamExt;

let mut text = String::new();
let mut stream = engine.stream_complete(prompt, &params).await?;

while let Some(tok) = stream.next().await {
    text.push_str(&tok?.text);
}
println!("{text}");
```

### 5-3. 조기 중단

스트림을 `drop`하면 생성이 즉시 중단됩니다. 처음 100 토큰만 받고 싶을 때는 다음과 같이 할 수 있습니다.

```rust
let mut stream = engine.stream_complete(prompt, &params).await?;
let mut count = 0;

while let Some(tok) = stream.next().await {
    print!("{}", tok?.text);
    count += 1;
    if count >= 100 {
        break; // 여기서 drop -> background task 자동 종료
    }
}
```

---

## 6. AgentRuntime 연동

에이전트 런타임에 LLM 엔진을 연결하면 `defense` -> `infer` -> `policy` -> `tool` -> `ZK proof`과 같습니다. 전체 파이프라인이 LLM 출력과 함께 동작합니다.

### 6-1. 빌더 설정

```rust
use std::sync::Arc;
use lumen_inference::{CandleLlmEngine, InferenceEngine, StreamingEngine};
use lumen_agent::AgentRuntime;

// 엔진 생성 (위 섹션 참고)
let engine = Arc::new(CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")?);

let runtime = AgentRuntime::builder(agent_id, policy)
    // InferenceEngine: step() 에서 사용
    .inference(engine.clone() as Arc<dyn InferenceEngine>)
    // StreamingEngine: stream_step()을 활성화하려면 반드시 설정
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

### 6-2. 일반 step (전체 응답 후 반환)

```rust
let result = runtime.step("echo hello world").await?;
println!("응답: {}", result.completion.text);
println!("도구 출력: {:?}", result.tool_output);
println!("검증: {:?}", result.verification);
```

### 6-3. 스트리밍 step (토큰 실시간 + 최종 결과)

```rust
use futures::StreamExt;
use lumen_agent::StreamEvent;

let mut events = runtime.stream_step("Rust 의 소유권(ownership) 모델을 설명해줘.").await?;

while let Some(event) = events.next().await {
    match event? {
        StreamEvent::Token(tok) => {
            // 토큰이 생성될 때마다 즉시 출력
            print!("{}", tok.text);
        }
        StreamEvent::Complete(result) => {
            // 스트림 종료: ZK proof, tool 결과 등 접근 가능
            println!("\n--- 생성 완료 ---");
            println!("Defense: {:?}", result.defense_verdict);
            println!("ZK 검증: {:?}", result.verification);
            if let Some(output) = result.tool_output {
                println!("도구 출력: {}", output);
            }
        }
    }
}
```

### 6-4. BackendConfig 팩토리 (간결한 설정)

```rust
use lumen_inference::backend::{BackendConfig, create_engine};

let engine = create_engine(BackendConfig::CandleLlm {
    handle, // VerifiedModelHandle
    tokenizer_path: "models/tokenizer.json".into(),
})?;

// engine: Arc<dyn InferenceEngine>
let runtime = AgentRuntime::builder(agent_id, policy)
    .inference(engine)
    // 스트리밍은 별도로 StreamingEngine 을 직접 설정해야 함
    ...
    .build()?;
```

> [!TIP]
> `create_engine`은 `Arc<dyn InferenceEngine>`을 반환하므로 스트리밍이
> 필요하면 `CandleLlmEngine::from_gguf`로 직접 생성 후 두 trait으로 각각 전달하세요.

---

## 7. SamplingParams 조정

| 파라미터                 | 기본값       | 설명                                |
|----------------------|-----------|-----------------------------------|
| `max_tokens`         | 256       | 생성할 최대 토큰 수                       |
| `temperature`        | 0.0       | 0.0 = greedy (결정론적), 높을수록 창의적     |
| `top_p`              | 1.0       | 0.9 등으로 설정하면 nucleus sampling 활성화 |
| `top_k`              | 0         | 상위 k 개 토큰만 후보로 유지. 0 = 비활성        |
| `repetition_penalty` | 1.0       | `1.1`–`1.3` 이면 반복 억제 효과           |
| `seed`               | `Some(0)` | `None`이면 매 실행마다 무작위 결과            |
| `stop_sequences`     | 빈 배열      | 해당 문자열 등장 시 생성 중단                 |

### 용도별 권장 설정

```rust
// 결정론적: 테스트, ZK proof 재현성
SamplingParams {
    temperature: 0.0,
    seed: Some(42),
    ..Default::default()
}

// 균형: 일반적인 대화
SamplingParams {
    max_tokens: 512,
    temperature: 0.7,
    top_p: 0.9,
    seed: None,
    ..Default::default()
}

// 창의적: 글쓰기, 브레인스토밍
SamplingParams {
    max_tokens: 1024,
    temperature: 1.2,
    top_p: 0.95,
    repetition_penalty: 1.1,
    seed: None,
    ..Default::default()
}

// 코드 생성: 낮은 온도, 반복 억제
SamplingParams {
    max_tokens: 512,
    temperature: 0.2,
    top_p: 0.95,
    stop_sequences: vec!["```".into()],
    ..Default::default()
}
```

> [!WARNING]
> **ZK 재현성 주의**: `temperature > 0`이면 출력이 비결정론적입니다.
> ZK witness에 바인딩하는 것은 **정수 라우팅 인덱스**(도구 선택 결정)이며, 생성된 텍스트 자체는 바인딩하지 마세요!

---

## 8. 양자화 수준 선택

`QuantizationConfig`는 백엔드에 양자화 의도를 전달하는 설정 타입입니다. 현재 `CandleLlmEngine`은 GGUF 파일 자체에 양자화가 인코딩되어 있으므로, **파일을 선택하는 것이 양자화 수준을 결정**합니다.

```rust
use lumen_inference::quantize::{QuantizationConfig, GgufLevel};

// Q4: 속도와 정확도의 균형 (가장 일반적)
let _q4 = QuantizationConfig::gguf_q4();  // GgufLevel::Q4K0

// Q8: 더 높은 정확도, 2배 메모리
let _q8 = QuantizationConfig::gguf_q8();  // GgufLevel::Q8K0

// ZK 경로: 도구 라우팅 점수를 고정소수점으로
let _zk = QuantizationConfig::zk_fixed(); // FixedPointPrecision::Q16_16
```

### GGUF 수준별 7B 모델 기준 비교

| 수준     | 파일 크기   | 최소 RAM | 속도    | 정확도 손실 |
|--------|---------|--------|-------|--------|
| Q4_K_M | ~4.1 GB | 6 GB   | 빠름    | 낮음     |
| Q5_K_M | ~4.8 GB | 7 GB   | 중간    | 매우 낮음  |
| Q8_0   | ~7.2 GB | 10 GB  | 느림    | 거의 없음  |
| F16    | ~14 GB  | 18 GB  | 가장 느림 | 없음     |

> [!NOTE]
> **현재 구현은 모든 추론이 CPU에서 실행됩니다.** GPU/Metal 지원은 추후 feature flag로 추가될 예정입니다.

---

## 9. Ed25519 서명 검증 (프로덕션)

정부 또는 규제 등 고보안 환경에서는 **신뢰된 서명자의 Ed25519 서명이 있는 모델**만 허용하세요.

### 9-1. 매니페스트 서명 (모델 배포자 측)

```rust
use lumen_core::SigningKey;
use lumen_provenance::ModelManifest;

// 서명 키 생성 (비밀키는 HSM 또는 Vault에 보관)
let signing_key = SigningKey::generate(&mut rand::thread_rng());
let verifying_key = signing_key.verifying_key();

// 매니페스트 서명
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

// TOML 로 직렬화해 배포
let toml_str = toml::to_string(&manifest)?;
std::fs::write("model_signed.toml", &toml_str)?;
```

### 9-2. 서명 검증 로더 (모델 사용자 측)

```rust
use lumen_core::VerifyingKey;
use lumen_inference::loader::VerifiedModelLoader;

// 신뢰된 서명자의 공개키 (사전 배포, 하드코딩 또는 정책 파일에 포함)
let trusted_key: VerifyingKey = /* ... */;
let loader = VerifiedModelLoader::new(vec![trusted_key]);

let toml_str = std::fs::read_to_string("model_signed.toml")?;
let manifest: lumen_provenance::ModelManifest = toml::from_str(&toml_str)?;

// BLAKE3 해시 + Ed25519 서명 모두 검증, 둘 중 하나라도 실패하면 에러
let handle = loader.load(manifest.path.as_path(), &manifest)?;
```

---

## 10. 성능과 메모리 가이드

### 첫 번째 추론이 느린 이유

`CandleLlmEngine::from_gguf`는 GGUF 파일 전체를 RAM에 로드합니다. 이 과정은 모델 크기에 따라 수 초가 걸릴 수 있습니다. 로딩 후에는 `Arc<Mutex<ModelWeights>>`로 공유하므로 **추가 비용이 없습니다.**

```rust
// 프로세스 시작 시 한 번만 로드, 이후 재사용
static ENGINE: once_cell::sync::OnceCell<Arc<CandleLlmEngine>> = OnceCell::new();

let engine = ENGINE.get_or_try_init(|| {
    // 이 블록은 최초 1회만 실행
    CandleLlmEngine::from_gguf(&handle, "models/tokenizer.json")
        .map(Arc::new)
})?;
```

### 동시 요청

`AgentRuntime`을 여러 태스크에서 공유할 때, 모델 잠금(`Mutex<ModelWeights>`)이 생성 중에 보유됩니다. 동시 LLM 요청은 직렬로 처리됩니다. 고(높은) 처리량이 필요하면 여러 엔진 인스턴스를 별도 태스크로 운용하세요.

### 토큰 생성 속도 목표 (CPU 기준)

| 모델 크기 | 양자화    | 예상 속도       |
|-------|--------|-------------|
| 1B    | Q8_0   | 20–40 tok/s |
| 3B    | Q4_K_M | 10–20 tok/s |
| 7B    | Q4_K_M | 5–12 tok/s  |
| 7B    | Q8_0   | 3–7 tok/s   |

> [!NOTE]
> CPU 코어 수, 클럭, 메모리 대역폭에 따라 크게 변동됩니다.

---

## 11. 자주 발생하는 오류

### `Error::Provenance("hash mismatch")`

> 모델 파일이 매니페스트의 해시와 다릅니다.

- 다운로드가 완전히 완료됐는지 확인하세요(`b3sum` 재계산).
- 매니페스트의 hex 문자열이 정확한지 확인하세요(64자).
- 파일이 수정되지 않았는지 확인하세요.

### `Error::Inference("GGUF 매직 불일치")`

> 파일이 GGUF 포맷이 아닙니다.

- `.gguf` 확장자라도 실제 포맷이 다를 수 있습니다.
- 다운로드를 재시도하거나 파일 헤더를 확인하세요: `xxd models/model.gguf | head -1`.
- 올바른 출력: `47475546` (`GGUF`).

### `Error::Inference("토크나이저 로드 실패")`

> `tokenizer.json` 경로가 잘못됐거나 모델과 맞지 않습니다.

- `tokenizer.json`이 해당 모델의 것인지 확인하세요.
- 경로가 현재 실행 디렉토리 기준으로 올바른지 확인하세요.

### `Error::NotImplemented("스트리밍 엔진이 설정되지 않았습니다")`

> `stream_step()`를 사용하려면 빌더에 `streaming_engine()`도 설정해야 합니다.

```rust
let engine = Arc::new(CandleLlmEngine::from_gguf(&handle, tokenizer)?);
AgentRuntime::builder(...)
    .inference(engine.clone() as Arc<dyn InferenceEngine>)
    .streaming_engine(engine as Arc<dyn StreamingEngine>) // 필수
    ...
```

### 메모리 부족 (OOM)

모델이 RAM을 초과합니다. 더 낮은 양자화 수준의 모델 파일을 사용하거나 더 작은 모델(1B–3B)로 교체하세요.

---

## 전체 동작 예제

다음은 모든 요소를 통합한 최소 동작 예제입니다.

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
    // 1. 모델 검증 및 엔진 생성
    let hash: Blake3Hash = "여기에_b3sum_출력_hex".parse()?;
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

    // 2. 에이전트 런타임 구성
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

    // 3. 스트리밍 실행
    println!("생성 중...");
    let mut events = runtime.stream_step("Rust 의 특징을 간단히 설명해줘.").await?;

    while let Some(event) = events.next().await {
        match event? {
            StreamEvent::Token(tok) => print!("{}", tok.text),
            StreamEvent::Complete(result) => {
                println!("\n\n[ZK 검증: {:?}]", result.verification);
            }
        }
    }

    Ok(())
}
```

---

## 다음 단계

- [lumen-inference 소스](crates/lumen-inference/src/): 백엔드 구현 코드
- [INTRODUCTION.md](INTRODUCTION.md): 전체 설계 철학과 보안 모델
- [llama-cpp feature](crates/lumen-inference/src/llama_cpp.rs): llama.cpp 백엔드(다음 마일스톤에서 완성 예정)
- 전체 파이프라인 데모인 `cargo run -p lumen-cli --example hello_agent` 실행해보기
