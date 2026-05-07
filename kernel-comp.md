# 폐쇄형(Air-Gapped) 환경 의존성 점검 보고서

> **목적**: Lumen을 iso-light-k0 마이크로커널 환경에서 동작하도록 개조하기 위해,
> 현재 사용 중인 외부 의존성을 전수 점검하고 교체/제거/유지 방침을 정리.

---

## 요약

| 분류 | 의존성 | 조치 |
|------|--------|------|
| **암호학 (교체 대상)** | `blake3` | elib-k0-nt/blake 로 교체 |
| | `ed25519-dalek` | elib-k0-nt/ed25519 로 교체 |
| | `subtle` | elib-k0-nt/constant-time 로 교체 |
| | `rand` + `rand_core` | elib-k0-nt/rng 로 교체 + 어댑터 작성 |
| | `aes-gcm` (crypto-channel) | elib-k0-nt/aes 로 교체 |
| | `x25519-dalek` (crypto-channel) | elib-k0-nt/x25519 로 교체 |
| **대형 외부 (평가 필요)** | `wasmtime` | 벤더링 유지 (대체재 없음) |
| | `halo2_proofs` + `ff` + `pasta_curves` | 현재 mock-only → 별도 결정 필요 |
| | `candle-*` | 벤더링 또는 llama.cpp 백엔드 전환 |
| | `tokenizers` | GGUF 내장 토크나이저로 제거 가능 |
| **유지** | 나머지 (serde, tokio 등) | 순수 Rust, 네트워크 무관 → 벤더링으로 유지 |

---

## 1. 교체 대상: 암호학 의존성

### 1-1. `blake3`

**현재 사용처**

| 위치 | 사용 내용 |
|------|-----------|
| `lumen-core/src/hash.rs` | `Blake3Hash::of()`, `Blake3Hash::of_file()` — 모델 파일 및 데이터 해시 |
| `lumen-channel/src/encrypted.rs:290` | `blake3::Hasher::new_keyed(shared)` — x25519 공유 비밀에서 방향별 AES 키 도출 (KDF) |
| `lumen-zkml` | 증명 witness commitment 해시 |
| `lumen-inference` | 프롬프트/모델 디제스트 tracing용 해시 |

**elib-k0-nt 대체**

`elib-k0-nt/blake` 에 `Blake3` 구현 존재. API 매핑:

```rust
// 기존: blake3::hash(data)
// 신규: Blake3::new().update(data).finalize()

// 기존: blake3::Hasher::new_keyed(&key).update(data).finalize()
// → elib-k0-nt/blake 에 keyed 모드 없음 → 추가 구현 필요 (아래 참고)
```

**추가 구현 필요 사항**

`elib-k0-nt/blake` 에 `Blake3::new_keyed(key: &[u8; 32])` 지원 추가 필요.
BLAKE3 스펙의 keyed hash 모드(domain separation context `"hash")는 입력 첫 블록에 키를 혼합하는 방식으로 구현되어 있음. 현재 `lumen-channel` 의 KDF 사용처 1곳이 이에 해당.

---

### 1-2. `ed25519-dalek`

**현재 사용처**

| 위치 | 사용 내용 |
|------|-----------|
| `lumen-core/src/crypto.rs` | `SigningKey`, `VerifyingKey` 래퍼 — 에이전트 ID 서명 키쌍 |
| `lumen-channel/src/encrypted.rs` | 핸드셰이크 서명/검증 |
| `lumen-capability` | Capability 토큰 서명/검증 |
| `lumen-provenance` | 모델 매니페스트 Ed25519 서명 검증 |

**elib-k0-nt 대체**

`elib-k0-nt/ed25519` 로 직접 교체. API 차이:

```rust
// 기존 키 생성: SigningKey::generate(&mut OsRng)
// 신규:
let mut seed = [0u8; 32];
rng.fill_bytes(&mut seed);                  // elib-k0-nt/rng 사용
let sk = SecretKey::from_bytes(&seed)?;
let pk = PublicKey::from(&sk);

// 기존 서명: sk.sign(msg)
// 신규: sign(msg, &sk)  →  Signature

// 기존 검증: pk.verify(msg, &sig)
// 신규: verify(msg, &sig, &pk)  →  Result<bool>
```

`lumen-core` 의 `SigningKey`/`VerifyingKey` 공개 타입 래퍼 유지 가능 (내부 구현만 교체).

---

### 1-3. `subtle`

**현재 사용처**

`lumen-core/src/hash.rs:56–58` 한 곳: `Blake3Hash` 의 타이밍 공격 방어용 상수시간 동등 비교.

```rust
// 기존: subtle::ConstantTimeEq::ct_eq(&self.0, &other.0).into()
// 신규: CtEqOps::eq(&a, &b).unwrap_u8() == 1  (elib-k0-nt/constant-time)
```

단일 사용처이므로 교체 난이도 낮음.

---

### 1-4. `rand` + `rand_core`

**현재 사용처**

| 위치 | 사용 내용 |
|------|-----------|
| `lumen-core/src/ids.rs` | `AgentId::random(&mut rng)`, `CapabilityId::random(&mut rng)` — `rand_core::RngCore` 트레이트 수용 |
| `lumen-core/src/crypto.rs` | `SigningKey::generate(&mut OsRng)` |
| `lumen-channel/src/encrypted.rs` | `OsRng` 기반 x25519 임시 키 생성, 논스 생성 |
| 다수 테스트/예제 | `OsRng` |

**elib-k0-nt 대체**

`elib-k0-nt/rng` 제공 API: `HashDRBGSHA256::new(entropy, nonce, personalization)` + `generate_fixed::<N>()`.

```rust
// OS 엔트로피 수집 (elib-k0-nt/rng)
let entropy = rng::os_entropy::get_entropy(64)?;
let mut drbg = HashDRBGSHA256::new(&entropy, &nonce, &personalization)?;
let key_bytes: [u8; 32] = drbg.generate_fixed()?;
```

**마이그레이션 시 주의 사항**

- `rand_core::RngCore` 트레이트를 수용하는 제네릭 함수들(`AgentId::random<R: RngCore>`)이 있어 트레이트 바운드를 변경하거나, 얇은 `RngCore` 어댑터를 구현해야 함.
- `rand` 크레이트 자체는 dev-only (`[dev-dependencies]`)로 쓰이는 곳이 많으므로, 프로덕션 코드에서는 `rand_core` 만 정리하면 됨.
- `rand_core::OsRng` 는 `elib-k0-nt/rng::os_entropy` 로 대체.

---

### 1-5. `aes-gcm` (feature: `crypto-channel`)

**현재 사용처**

`lumen-channel/src/encrypted.rs` 전용:
- `Aes256Gcm::new(key)` — tx/rx 방향별 사이퍼 초기화
- `cipher.encrypt(nonce, payload)` / `cipher.decrypt(nonce, payload)` — 프레임 암복호화
- `aes-gcm` 은 태그를 암호문 뒤에 붙여 반환

**elib-k0-nt 대체**

`elib-k0-nt/aes` 의 `AES256GCM` 사용. API 차이 (태그 분리):

```rust
// 기존: let ciphertext_with_tag = cipher.encrypt(nonce, Payload { msg, aad })?;
// 신규:
let mut ciphertext = vec![0u8; plaintext.len()];
let mut tag = [0u8; 16];
AES256GCM::new(key).encrypt(nonce, aad, plaintext, &mut ciphertext, &mut tag)?;
// 프레임 직렬화 시 ciphertext || tag 로 직접 이어붙이면 동일 포맷 유지 가능
```

---

### 1-6. `x25519-dalek` (feature: `crypto-channel`)

**현재 사용처**

`lumen-channel/src/encrypted.rs:137`:
```rust
let my_eph = EphemeralSecret::random_from_rng(OsRng);
let my_pub = X25519Public::from(&my_eph);
let shared = my_eph.diffie_hellman(&their_pub);
```

**elib-k0-nt 대체**

`elib-k0-nt/x25519` 직접 교체:

```rust
// 임시 키 생성: rng로 32바이트 생성 후 SecretKey 래핑
let eph_bytes: [u8; 32] = drbg.generate_fixed()?;
let my_eph = x25519::SecretKey::from_bytes(eph_bytes);
let my_pub = my_eph.public_key();
let shared: SharedSecret = my_eph.diffie_hellman(&their_pub);
// shared.expose() 로 [u8;32] 접근 가능
```

`EphemeralSecret` 타입 소멸 후 키 파기는 elib-k0-nt `SharedSecret` 의 자동 zeroize로 동일하게 보장됨.

---

## 2. 평가 대상: 대형 외부 의존성

### 2-1. `wasmtime`

**현재 사용처**

`lumen-sandbox` 전용. 에이전트 코드를 WASM으로 격리 실행하는 핵심 보안 경계.

- `Engine::new()` — 결정론적 설정 (fuel, epoch, Cranelift JIT)
- `Module::new()` — WASM 바이트코드 컴파일
- `Store::new()` + `set_fuel()` / `set_epoch_deadline()` — 실행 한계 강제
- `Linker::instantiate_async()` — 격리 인스턴스 생성
- `instance.get_func().call_async()` — 진입점 호출

**폐쇄형 환경 문제점**

- 네트워크 비의존적(런타임 무관), 하지만 Cranelift JIT 컴파일러 포함으로 소스 트리가 방대함 (~100개 이상 크레이트).
- 커널 Ring 0/1에서는 JIT 실행 불가 — Ring 3 또는 별도 프로세스에서만 사용 가능.

**대안 검토**

| 대안 | 장점 | 단점 |
|------|------|------|
| `wasmi` v0.31+ | 순수 Rust 인터프리터, 소스 트리 소형 | 성능 10–50배 저하, fuel API 상이 |
| `wasm3-sys` | 경량 C 인터프리터 | C 바인딩 → 커널 no_std 불가 |
| wasmtime 벤더링 | 현재 보안 속성 그대로 유지 | 벤더 소스 관리 부담 (~15 MB) |

**권고**: WASM 샌드박스는 보안의 핵심이므로 wasmtime을 벤더링하여 유지. `wasmi` 로의 전환은 성능 허용 시 별도 검토. 커널 통합 시 Ring 3 사용자공간 프로세스에서 실행하는 구조 필요.

---

### 2-2. `halo2_proofs` + `ff` + `pasta_curves`

**현재 사용처**

`lumen-zkml` 의 `halo2` feature 뒤에서만 활성화.

- `MockProver::verify()` — ZK 회로 제약 충족 여부 검증 (증명 비공개 X, witness 노출)
- `RoutingProofCircuit` — 에이전트 툴 라우팅 결정의 PLONKish 회로 정의
- 현재 v0.3 기준으로 간결한 ZK(succinct proof)는 구현되지 않음

**폐쇄형 환경 문제점**

- `pasta_curves` 등 타원곡선 연산 의존성이 크고 빌드 시간이 매우 긺.
- MockProver는 실제 ZK가 아니어서 witness가 노출됨 → 보안 기여 미미.
- 네트워크 비의존적이지만 벤더링 크기 상당.

**직접 구현 방향 (mock 대체)**

현재 MockProver 사용처는 "회로 제약 만족 여부 검증"에 불과. halo2 없이 대체 가능:

```rust
// RoutingProofCircuit의 제약 조건들을 순수 단언(assertion) 기반으로 직접 검증
// → lumen-zkml의 mock 프루버를 halo2 불필요 버전으로 재작성
// BLAKE3 기반 commitment만 유지 (elib-k0-nt/blake)
```

실제 succinct ZK가 필요한 v0.4 시점에는 별도 프레임워크(SP1, RISC Zero 등) 선택 필요.

**권고**: `halo2` feature를 기본에서 제거하고, mock prover를 halo2 불필요 버전으로 재구현. 실제 ZK는 마일스톤 재검토 시 결정.

---

### 2-3. `candle-core` + `candle-onnx` + `candle-transformers`

**현재 사용처**

`lumen-inference` 의 `candle` / `candle-llm` feature 뒤에서만 활성화.

- `candle-core`: `Device::Cpu`, `Tensor` 연산 (CPU 전용)
- `candle-transformers`: `ModelWeights` (GGUF 양자화 LLaMA), `LogitsProcessor` 샘플링
- `candle-onnx`: ONNX 모델 라우팅 엔진

**폐쇄형 환경 문제점**

- 런타임 네트워크 비의존적이나, candle은 HuggingFace Hub 다운로드 예제 코드 포함 → **소스 벤더링 시 해당 부분 제거 필요**.
- `rayon` (병렬 연산), `blas`/`metal` 선택적 의존 → 커널 환경에서는 단일 스레드 강제 필요.
- 빌드 의존 크레이트 수 매우 많음.

**대안**

| 대안 | 비고 |
|------|------|
| `llama-cpp-2` (C 바인딩) | 이미 `lumen-inference/src/llama_cpp.rs` 스텁 존재. GGUF 내장 토크나이저 지원. |
| candle 소스 벤더링 | CPU-only feature 강제, HF Hub 코드 제거 필요 |
| 자체 GGUF 파서 + 추론 | 구현 비용 매우 높음 |

**권고**: llama.cpp 백엔드(`llama-cpp-2`)를 우선 완성. air-gapped에서는 C 코드가 허용된다면 llama.cpp가 더 성숙하고 가벼움. candle은 소스 벤더링 후 CPU-only로 유지하는 것도 가능.

---

### 2-4. `tokenizers` (HuggingFace)

**현재 사용처**

`lumen-inference/src/candle_llm.rs:86`:
```rust
let tokenizer = Tokenizer::from_file(tokenizer_path)  // HF tokenizer.json 로컬 파일
```

- BPE/WordPiece 토크나이저를 `tokenizer.json` 형식으로 로드
- `encode()` / `decode()` — 텍스트 ↔ 토큰 ID 변환
- EOS 토큰 감지 (`</s>`, `<|endoftext|>`)

**폐쇄형 환경 문제점**

- `onig` feature: C 소스(Oniguruma 정규식)를 소스에서 빌드 → 커널 no_std 환경 불가.
- 대형 dep tree (rayon, serde_json, unicode 등).
- **GGUF 모델 파일에는 토크나이저 어휘/머지 규칙이 내장되어 있음** — `tokenizer.json` 불필요.

**직접 구현 방향**

GGUF 형식(`gguf-rs` 또는 직접 파서)에서 어휘를 추출 후 최소 BPE 토크나이저 구현:

```
GGUF 헤더 → tokenizer.ggml.* 메타데이터 추출
  → vocab (id → str), merges (str pair → id) 로딩
  → encode(): pre-tokenize → BPE 머지 반복
  → decode(): id → str 매핑
```

LLaMA 3 / Mistral 계열은 SentencePiece BPE, GPT-2 계열은 Byte-BPE.
약 300–500 LoC 순수 Rust 구현 가능 (no_std 호환).

**권고**: `tokenizers` 제거하고 GGUF 내장 어휘 기반 최소 BPE 토크나이저를 `lumen-inference` 에 직접 구현. llama.cpp 백엔드 사용 시 해당 백엔드가 토크나이저를 내장 처리하므로 불필요.

---

## 3. 유지 대상 (문제없는 의존성)

아래 의존성들은 순수 Rust, 네트워크 무관하며 직접 구현 비용이 유지 비용을 초과. **소스 벤더링**으로 유지.

| 의존성 | 이유 |
|--------|------|
| `tokio` | 비동기 런타임. OS API만 사용. 커널 Ring 3 프로세스에서 동작 가능. |
| `async-trait`, `futures` | 순수 Rust 비동기 추상화. |
| `serde`, `serde_json`, `toml`, `postcard` | 직렬화. 네트워크 무관. |
| `hex` | 16진수 인코딩. 60 LoC 대체 가능하나 유지가 합리적. |
| `thiserror`, `anyhow` | 에러 추상화. 순수 Rust. |
| `aho-corasick`, `regex` | 프롬프트 인젝션 방어 패턴 매칭. 순수 Rust, 네트워크 무관. |
| `globset` | 파일 경로 Capability 매칭. 순수 Rust. |
| `once_cell` | 정적 초기화. 순수 Rust. |
| `parking_lot` | 동시성 기본기. 순수 Rust. |
| `safetensors` | 모델 헤더 읽기 전용. 네트워크 무관. 순수 Rust. |
| `clap` | CLI 전용. 커널 내부 로직 불관여. |
| `tracing`, `tracing-subscriber` | 감사 로그. 순수 Rust. |
| `criterion` | dev-only 벤치마크. |
| `wat` | dev-only WASM 텍스트 변환. |
| `safetensors` | 테스트 픽스처 생성 전용 (프로덕션 불사용). |

---

## 4. 마이그레이션 우선순위

### Phase 1 — 암호학 교체 (즉시 착수)

교체 자체가 API 레벨 수정에 그침. elib-k0-nt 모듈로 1:1 대체.

1. `subtle` → `elib-k0-nt/constant-time` (`lumen-core/src/hash.rs` 1곳)
2. `blake3` → `elib-k0-nt/blake` (keyed 모드 추가 필요)
3. `ed25519-dalek` → `elib-k0-nt/ed25519`
4. `rand` + `rand_core` → `elib-k0-nt/rng` + `RngCore` 어댑터 작성
5. `aes-gcm` → `elib-k0-nt/aes` (crypto-channel feature)
6. `x25519-dalek` → `elib-k0-nt/x25519` (crypto-channel feature)

### Phase 2 — 추론 백엔드 정비

1. `tokenizers` 제거 → GGUF 내장 BPE 토크나이저 직접 구현 또는 llama.cpp 백엔드 전환
2. candle 의존성을 llama.cpp 백엔드로 대체 (또는 소스 벤더링 후 CPU-only 강제)

### Phase 3 — ZK 정리 및 WASM 벤더링

1. `halo2` feature 기본 비활성화 + mock prover 독립 재구현 (halo2_proofs 제거)
2. `wasmtime` 전체 소스 벤더링 + HF Hub 참조 제거

### Phase 4 — 나머지 의존성 벤더링

나머지 "유지 대상" 의존성들을 오프라인 소스 벤더링(`cargo vendor`)으로 전환. `iso-light-k0` 빌드 시스템에 통합.

---

## 5. elib-k0-nt 미지원 기능 (신규 구현 필요)

| 필요 기능 | 현재 대응 | 구현 방향 |
|-----------|-----------|-----------|
| **BLAKE3 keyed 해시** | `blake3::Hasher::new_keyed()` | elib-k0-nt/blake에 `Blake3::new_keyed(key: &[u8;32])` 추가 |
| **RngCore 어댑터** | `rand_core::RngCore` 트레이트 | `elib-k0-nt/rng::HashDRBG`를 `RngCore` 구현체로 감싸는 어댑터 (Lumen 내부 구현) |
| **GGUF 토크나이저** | `tokenizers` crate | `lumen-inference` 에 최소 BPE 구현 (~400 LoC, no_std 가능) |
| **Halo2-free mock prover** | `halo2_proofs::MockProver` | 제약 조건 직접 평가 로직으로 대체 (BLAKE3 commitment 유지) |
