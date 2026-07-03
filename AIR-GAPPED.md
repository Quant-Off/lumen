# 폐쇄형(Air-Gapped) 환경 빌드 가이드

> **목적**: Lumen 을 외부 네트워크가 차단된 고보안 구역(High-Side)에서
> 빌드/테스트/배포할 수 있도록 의존성을 사전 벤더링하고 오프라인
> 4-게이트를 통과시키는 절차를 정의합니다. iso-light-k0 마이크로커널의
> Ring 3 사용자공간 컴포넌트로 통합되는 시점부터 본 가이드의 절차가
> 빌드 시스템에 강제됩니다.

---

## 1. 사전 정리 — v0.4 의존성 변경 사항

폐쇄망 호환을 위해 v0.4 에서 다음 의존성이 제거되었습니다:

| 제거 의존성 | 사유 | 대체 |
|-------------|------|------|
| `blake3`, `ed25519-dalek`, `subtle`, `rand`, `rand_core`, `aes-gcm`, `x25519-dalek` | 외부 crates.io 의존, FIPS 미인증 | `elib-k0-nt/*` (in-house, 인증 경로) |
| `halo2_proofs` + `ff` + `pasta_curves` | 타원곡선 의존성 ~30 크레이트, MockProver 는 ZK 보장 없음 | mock prover 가 BLAKE3 commitment 만 사용. succinct ZK 는 SP1/RISC Zero 마일스톤에서 재검토. |
| `candle-transformers` + `tokenizers` | HuggingFace Hub 다운로드 코드 포함, onig C 빌드 의존 | `llama-cpp` 백엔드 (v0.5) 또는 호스트 TEE 의 `ChannelEngine` forward 경로 |

자세한 사유는 [`kernel-comp.md`](kernel-comp.md) 참고.

---

## 2. 의존성 벤더링 (온라인 단계)

폐쇄망 이전을 위해 온라인 환경에서 한 번 수행합니다.

```bash
# 워크스페이스 루트에서:
./scripts/vendor.sh           # default features 만 (가장 가벼움)
./scripts/vendor.sh --all     # crypto-channel + llama-cpp 까지 포함
```

스크립트가 수행하는 작업:

1. `cargo vendor` 로 `Cargo.lock` 의 모든 크레이트 소스를 `vendor/` 에 복제
2. `.cargo/vendor-config.toml` → `.cargo/config.toml` 활성화 (소스 교체 + `net.offline = true`)
3. 산출물 크기/개수 출력

산출물:

| 경로 | 크기 | git | 비고 |
|------|------|-----|------|
| `vendor/` | ~500MB–1GB | ❌ ignored | `cargo vendor` 산출물 |
| `.cargo/config.toml` | ~700B | ❌ ignored | 활성화된 source-replacement 설정 |
| `.cargo/vendor-config.toml` | ~700B | ✅ committed | 템플릿 |

---

## 3. 시스템 바이너리 사전 패키징

워크스페이스 외부의 빌드 도구는 별도로 폐쇄망에 옮겨야 합니다.

| 바이너리 | 활성화 조건 | 용도 |
|----------|-------------|------|
| `rustc` 1.85.0 + cargo | 항상 (`rust-toolchain.toml`) | Rust 컴파일러 |
| `protoc` (Protocol Buffers) | `lumen-inference/candle` feature | `candle-onnx` 의 build script 가 빌드 시점에 호출 |
| `cmake` ≥ 3.20 | `lumen-inference/llama-cpp` feature (v0.5) | llama.cpp 네이티브 라이브러리 빌드 |

폐쇄망 반입 시 권장: `~/.cargo/`, `protoc` 바이너리, `cmake`, 그리고 본
저장소(벤더링 완료 상태)를 함께 묶어 `tar.gz` 단일 번들로 전송.

```bash
# 온라인 측에서:
./scripts/vendor.sh --all
tar czf lumen-airgap-bundle.tar.gz \
    --exclude=target --exclude=.git \
    .

# 폐쇄망 측에서 (예: 데이터 다이오드/CDS 통과 후):
tar xzf lumen-airgap-bundle.tar.gz
```

---

## 4. 오프라인 4-게이트 검증

폐쇄망에서 다음 명령이 모두 통과해야 합니다.

```bash
cargo build  --offline --workspace --all-targets
cargo test   --offline --workspace
cargo clippy --offline --workspace --all-targets -- -D warnings
cargo fmt    --all -- --check
```

`vendor-config.toml` 의 `net.offline = true` 설정 덕분에 외부 fetch 시도가
있으면 빌드가 즉시 실패합니다 — 폐쇄망 정책 위반을 컴파일 시점에
탐지하는 안전장치입니다.

선택적 feature 별 검증:

```bash
# 보안 채널 (X25519 + AES-256-GCM, elib-k0-nt 백엔드)
cargo test --offline --workspace --features lumen-channel/crypto-channel

# llama.cpp 추론 백엔드 (v0.5 스텁)
cargo test --offline --workspace --features lumen-inference/llama-cpp

# ONNX 라우팅 추론 (protoc 시스템 바이너리 필요)
cargo build --offline --workspace --features lumen-inference/candle
```

---

## 5. iso-light-k0 마이크로커널 통합 메모

Lumen 은 iso-light-k0 의 Ring 3 사용자공간 컴포넌트로 동작합니다.
보안 경계는 다음과 같습니다.

```
┌─────────────────────────── High-Side (Air-Gapped) ───────────────────────────┐
│                                                                              │
│  Ring 0  ┃ iso-light-k0 마이크로커널 (PSK 기반 mutual auth, TLS-PSK-PQ)        │
│          ┃   - elib-k0-nt 암호 모듈 (FIPS 인증 경로)                          │
│  ─────────┃─────────────────────────────────────────────────────────────────  │
│  Ring 3  ┃ Lumen 에이전트 프레임워크 (본 저장소)                              │
│          ┃   - WASM 샌드박스 (wasmtime, JIT)                                 │
│          ┃   - 호스트 TEE 추론 forward (ChannelEngine, AES-256-GCM)          │
│          ┃   - 결정론적 상태 (tokio + lumen-fixed)                           │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

**제약 사항:**
- **Ring 0 에서 Cranelift JIT 실행 불가** — 따라서 `lumen-sandbox` 는
  반드시 Ring 3 사용자공간 프로세스로 실행. 커널-에이전트 간 통신은
  iso-light-k0 의 syscall 또는 PSK 기반 IPC 채널 사용.
- **모든 외부 fetch 금지** — `cargo build` 의 네트워크 시도조차
  `net.offline = true` 로 차단. crates.io 미러 운용 금지.
- **모델 가중치 외부 반입** — Safetensors / ONNX / GGUF 파일은
  `lumen-provenance` 의 BLAKE3 + Ed25519 매니페스트 검증을 통과해야만
  Ring 3 에서 로드 가능.

---

## 6. 운영 점검 (정기)

| 점검 항목 | 주기 | 명령 |
|-----------|------|------|
| `vendor/` 무결성 | 빌드 전 | `./scripts/vendor.sh --check` |
| 4-게이트 | 매 변경 | 위 §4 참고 |
| `Cargo.lock` 변동 | PR 검토 | `git diff Cargo.lock` |
| 시스템 바이너리 버전 | 분기 | `rustc --version`, `protoc --version` |

---

## 7. 트러블슈팅

### `error: failed to get ... from registry`

→ `.cargo/config.toml` 의 `vendored-sources` 가 활성화되지 않았거나
   `vendor/` 가 누락. `./scripts/vendor.sh` 재실행.

### `error: Could not find protoc`

→ `lumen-inference/candle` feature 사용 시 발생. 폐쇄망에 `protoc` 바이너리
   사전 배치 필요. `PROTOC=/path/to/protoc cargo build ...` 로 명시 가능.

### `error: vendor/ 디렉토리가 없습니다`

→ `./scripts/vendor.sh --check` 를 vendor 디렉토리 없이 실행한 경우.
   먼저 `./scripts/vendor.sh` 또는 `./scripts/vendor.sh --all` 실행.

### 빌드가 오래 걸림

→ `wasmtime` (Cranelift JIT) 빌드가 ~2–3 분 소요. 정상.
   `cargo build --offline --jobs $(nproc)` 로 병렬화 권장.

---

## 참고

- [`kernel-comp.md`](kernel-comp.md) — 의존성 점검 보고서
- [`CLAUDE.md`](CLAUDE.md) — 프로젝트 헌장 (4-게이트, 한국어 docstring 등)
- [iso-light-k0 AIR-GAP.md](../iso-light-k0/AIR-GAP.md) — 커널 측 폐쇄망 프로토콜
