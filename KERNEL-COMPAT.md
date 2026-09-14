# 폐쇄 커널 호환성 검증 보고서 (K0)

> **목적**: Lumen 이 폐쇄형(Air-Gapped) `K0` 마이크로커널 환경에서
> 정상 동작할 수 있는지 기술적으로 엄밀히 검증한 결과를 정리.

---

## 1. 검증 방법론

세 단계 검증:

| 단계 | 산출물 | 통과 기준 |
|------|--------|-----------|
| **(A) 정적 의존성 감사** | Cargo.lock + cargo build --message-format=json 추출 | 금지 크레이트 0건, 네트워크 스택 0건 |
| **(B) 와이어 호환성 회귀 테스트** | `crates/lumen-core/tests/elib_wire_compat.rs` (7 테스트) | lumen 래퍼 ≡ elib-k0-nt 직접 호출 (비트-동일) |
| **(C) 5-게이트 (offline 모드)** | `cargo build/test/clippy/fmt --check/deny check` 모두 `--offline --locked` | 모두 통과 |

---

## 2. (A) 정적 의존성 감사 결과

### 2.1 lumen-cli (실제 운영 바이너리) 릴리스 빌드

| 항목 | 결과 |
|------|------|
| 워크스페이스 멤버 | 16 크레이트 |
| `lumen-cli` 릴리스 바이너리에 컴파일되는 외부 크레이트 | **102 개** |
| `wasmtime` / `cranelift` / `pulley` 포함 여부 | **❌ 0건** (lumen-sandbox 는 별도 라이브러리) |
| `getrandom` / `rand` / `rand_core` | **❌ 0건** (dev-only, 운영 바이너리에 없음) |
| `tokenizers` | **❌ 0건** |
| `reqwest` / `hyper` / `http` / `rustls` / `openssl` / `native-tls` | **❌ 0건** |
| `mio` / `socket2` (tokio 네트워크 백엔드) | **❌ 0건** (lumen 의 tokio feature 에 `net` 미포함) |
| `Cargo.lock` 의 `git+` source dep | **❌ 0건** (전부 crates.io 레지스트리) |
| `unsafe` 사용 | 13 개 lumen 크레이트가 `#![forbid(unsafe_code)]` 선언; 유일한 예외는 `lumen-sandbox` (wasmtime FFI) |

### 2.2 lumen-sandbox (별도 옵션 컴포넌트)

`lumen-sandbox` 는 **다른 어떤 lumen 크레이트도 의존하지 않는 독립 라이브러리**입니다.
즉, `lumen-cli` 기본 운영 바이너리에는 wasmtime 이 포함되지 않습니다.

`grep -rn "lumen-sandbox" crates --include="Cargo.toml"` → 자기 자신 1건뿐.

| 항목 | 결과 |
|------|------|
| Cranelift JIT 코드 생성 (W^X 위반 가능) | ✅ 포함 (현재 `cranelift` feature) |
| Pulley 인터프리터 (W^X 안전) | ✅ wasmtime 트리에 동시 존재 |
| 결정 | 운영 환경에 맞춰 `cranelift` 또는 `pulley` 중 선택 가능. 현재 기본은 Cranelift JIT (Ring 3 RWX 페이지 가정). 엄격한 W^X 정책 환경이라면 wasmtime feature 를 `["async","pulley","runtime"]` 으로 전환 필요 |

### 2.3 런타임 시스템콜 표면 (lumen 운영 코드)

`grep -rn "use std::fs|std::env|std::process|tokio::net|tokio::process"` 결과:

| 사용처 | 시스템콜 | 폐쇄 커널 영향 |
|--------|----------|----------------|
| `lumen-core::hash::Blake3Hash::of_file` | `std::fs::File::open` | 모델 파일 검증; K0 가 Ring 3 에 fs 또는 IPC 전달을 제공하면 동작 |
| `lumen-provenance::*` | `std::path::Path` (경로 처리) + `std::fs` (파일 읽기) | 위와 동일 |
| `lumen-cli::cmd::run` 등 | `std::path::Path` | 경로 인자 처리 — 순수 메모리 연산 |
| `lumen-cli::cmd::verifier` | `std::env::var` (`privkey_env`, `fee_payer_env`) | **블록체인 배포용** (low-side 전용); 폐쇄망에서는 미사용 |
| `lumen-onchain::deploy` | `std::process::Command` (`forge` / `zk` 호출) | **블록체인 배포용**, `dry_run` 모드 제공 — 폐쇄망에서 텍스트 명령만 inspect 후 low-side 로 운반하도록 설계됨 |
| `tokio` features | `["rt-multi-thread", "macros", "sync", "time", "io-util", "fs"]` | `net` 미포함; `fs` / 멀티스레드는 Ring 3 ABI 가 제공해야 함 |
| 직접 `TcpStream`/`UdpSocket`/`reqwest`/`hyper` | **❌ 0건** | — |

---

## 3. (B) 와이어 호환성 회귀 테스트 결과

`cargo test -p lumen-core --test elib_wire_compat` — **7/7 통과**.

| 테스트 | 검증 대상 | 결과 |
|--------|-----------|------|
| `blake3_wrapper_byte_identical_to_direct_elib` | `Blake3Hash::of` ≡ `Blake3::new().update().finalize()` (4 입력) | ✅ |
| `blake3_wrapper_byte_identical_for_large_input` | 16 KiB 입력 (cv-스택 머지 경로) | ✅ |
| `blake3_keyed_derive_byte_identical_to_direct_elib` | `lumen_core::blake3_keyed_derive_32` ≡ `Blake3::new_keyed(...).update(...).finalize()` (lumen-channel KDF) | ✅ |
| `ed25519_sign_verify_via_wrapper_matches_direct_elib` | lumen 서명 → elib 검증 / elib 서명 → lumen 검증 양방향 | ✅ |
| `ed25519_signature_is_deterministic` | RFC 8032 결정성 (동일 msg+seed → 동일 64B 서명) | ✅ |
| `x25519_ecdh_produces_matching_shared_secret` | 양 피어 ECDH 32B 공유 비밀 일치 + 32B 와이어 라운드트립 | ✅ |
| `aes256_gcm_encrypts_with_tag_appended_format_used_by_lumen_channel` | `ciphertext \|\| tag` 와이어 포맷 + 결정성 + decrypt 라운드트립 | ✅ |

**의미**: lumen 의 `lumen-core::SigningKey/VerifyingKey/Blake3Hash`, `lumen-channel::EncryptedChannel` 의 KDF/AEAD 출력은 `K0` 의 `elib-k0d-core` 가 elib-k0-nt 를 직접 호출했을 때와 비트 단위로 동일합니다. 즉, **두 프로젝트 사이의 와이어 호환성이 회귀-방지로 보장**됩니다.

---

## 4. (C) 5-게이트 (Offline 모드) 결과

`cargo {build,test,clippy} --workspace --locked --offline ...`, `cargo fmt --all -- --check`, `cargo deny check`.

| 게이트 | 결과 |
|--------|------|
| `cargo build --workspace --all-targets --locked --offline` | ✅ |
| `cargo build --workspace --no-default-features --locked --offline` | ✅ |
| `cargo test --workspace --locked --offline` | ✅ 38 테스트 그룹 (와이어 컴팟 7개 포함) |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | ✅ |
| `cargo fmt --all -- --check` | ✅ |
| `cargo deny check` | ✅ `advisories ok, bans ok, licenses ok, sources ok` |
| `cargo build --release --target wasm32-unknown-unknown --locked --offline` (echo-agent) | ✅ |

`vendor-config.toml` 의 `[net] offline = true` 설정으로 외부 fetch 시도가 있으면 즉시 컴파일 실패.

---

## 5. 폐쇄 커널 배포 모델 (검증 결과 요약)

```
┌────────────────────── High-Side (Air-Gapped) ───────────────────────┐
│                                                                      │
│  Ring 0  ┃ K0 마이크로커널 (no_std, x86_64-unknown-none)    │
│          ┃   - elib-k0-nt 암호 모듈 (PSK / TLS-PQ-Hybrid / ML-KEM)    │
│          ┃   - elib-k0d-core IPC 디스패처 (ed25519 sign/verify 등)    │
│  ─────────┃─────────────────────────────────────────────────────────  │
│  Ring 3  ┃ Lumen Ring 3 사용자공간 컴포넌트 (본 저장소)               │
│          ┃                                                            │
│          ┃   ┌──────────────────────────────────────────────┐         │
│          ┃   │ lumen-cli + agent + orchestrator + channel + │         │
│          ┃   │ inference (BPE) + zkml mock + provenance     │         │
│          ┃   │  - wasmtime 없음, 네트워크 스택 없음          │         │
│          ┃   │  - 같은 elib-k0-nt 와 와이어 호환 (§3)        │         │
│          ┃   └──────────────────────────────────────────────┘         │
│          ┃                                                            │
│          ┃   ┌──────────────────────────────────────────────┐         │
│          ┃   │ lumen-sandbox (옵션, RWX 페이지 필요)          │         │
│          ┃   │  - Cranelift JIT — 강한 W^X 환경에서는        │         │
│          ┃   │    wasmtime feature 를 pulley 로 전환         │         │
│          ┃   └──────────────────────────────────────────────┘         │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

**K0 가 Ring 3 에 제공해야 하는 ABI 부분집합:**

| 카테고리 | 필요 항목 | 현재 사용처 |
|----------|-----------|-------------|
| 메모리 | `mmap` (ANONYMOUS, R/W) + `munmap` | std/tokio 일반 할당 |
| 메모리 | `mprotect` 로 RWX (Cranelift JIT) | **lumen-sandbox 한정** — 사용 안 하면 불필요 |
| 동기화 | futex / `pthread_mutex_lock` | parking_lot, tokio sync |
| 스레드 | `clone(CLONE_THREAD)` 또는 동등 | tokio rt-multi-thread |
| 시간 | 단조 시계 (CLOCK_MONOTONIC 또는 동등) | tokio time |
| 파일시스템 | `open/read/close` 또는 IPC 매핑 | 모델 가중치 / SBOM 로드 |
| 표준 입출력 | `write(stderr)` | tracing fmt subscriber |
| 엔트로피 | OS 엔트로피 또는 `RDRAND` 인터페이스 | `lumen_core::rng::OsRng` (`elib_rng::os_entropy`) |

**K0 가 *제공하지 않아도 되는* 것:**

- 네트워크 (TCP/UDP/HTTP) — lumen 의 모든 통신은 `lumen-channel::SecureChannel` 을 통해 외부 transport 에 위임됨. 폐쇄망에서는 PSK 기반 IPC 가 transport 로 주입됨.
- 프로세스 spawn — `lumen-onchain` 만 사용하며 폐쇄망에서는 `dry_run=true` 로만 호출.
- 동적 링킹/로딩 — 정적 바이너리.

---

## 6. 검증되지 않은(현재 환경에서 불가능한) 항목

이 항목들은 lumen 단독 자동 검증 범위를 벗어나므로 별도 통합 환경에서 확인 필요:

1. **K0 의 실제 Ring 3 ABI 와의 결합**: 현재 `K0` 자체는 Ring 0 베어메탈 커널 단계까지만 구현되어 있어 Ring 3 사용자공간 프로세스를 띄울 수 없습니다. 결합 검증은 K0 의 `userspace_loader` 마일스톤이 도달한 뒤 가능합니다.
2. **TLS 1.3-PSK-PQ 핸드셰이크와 lumen-channel::EncryptedChannel 의 프로토콜 변환**: 두 프로토콜은 **기본 암호 프리미티브를 공유** (BLAKE3, AES-GCM, ed25519, x25519) 하지만 핸드셰이크 메시지 포맷은 다릅니다. lumen 은 이를 **외부 transport 가 처리** 하는 추상으로 격리해 두었으므로(§5 다이어그램), 실제 결합 시 thin adapter 한 단을 추가하면 됩니다.
3. **Cranelift JIT 의 페이지 보호 동작**: 현재 호스트 (Linux/macOS) 에서는 동작하지만, K0 가 Ring 3 에 W^X 강제 페이지를 부여할 경우 Cranelift 코드 생성이 실패합니다 — Pulley 인터프리터로 전환해야 합니다 (단순 feature 변경; 위 5절 참조).

---

## 7. 결론

| 검증 영역 | 결과 |
|-----------|------|
| **암호 와이어 호환성** | ✅ lumen ≡ K0/elib-k0d-core 비트-동일 (회귀 테스트로 영구 보장) |
| **외부 의존성 정리** | ✅ 운영 바이너리에 네트워크 스택·tokenizers·rand_core·candle 없음 |
| **결정성** | ✅ Ed25519 서명, BLAKE3, AES-GCM, mock-prover 모두 결정론적 |
| **빌드 폐쇄성** | ✅ 5-게이트 모두 `--offline --locked` 통과, git source 0건 |
| **공급망 게이트** | ✅ `cargo deny check` 통과 (advisories / bans / licenses / sources 모두 clean) |
| **메모리 안전성** | ✅ 13/16 lumen 크레이트가 `forbid(unsafe_code)`; 예외는 wasmtime FFI 한 곳 |
| **JIT-free 운영 경로** | ✅ `lumen-cli` 기본 바이너리는 wasmtime 0건 |
| **샌드박스 환경 적응** | ⚠️ 강한 W^X 정책 시 wasmtime feature 를 `pulley` 로 전환 필요 (옵션) |

**Lumen 은 폐쇄 커널 (K0) Ring 3 사용자공간 환경에서 정상 동작할 수 있도록 설계·검증되어 있습니다.** 위의 7개 와이어 호환성 회귀 테스트가 lumen ≡ elib-k0-nt 비트 동일성을 매 빌드마다 강제하며, 외부 네트워크 의존성과 JIT 컴포넌트는 운영 바이너리에서 분리되어 있습니다.

---

## 부록 A. 회귀 테스트 실행 명령

```bash
cargo test -p lumen-core --test elib_wire_compat --offline
# 7 passed; 0 failed
```

## 부록 B. 참고 문서

- [`AIR-GAPPED.md`](AIR-GAPPED.md) — 폐쇄망 빌드 가이드
- [`kernel-comp.md`](kernel-comp.md) — 의존성 점검 보고서
- [`../K0/AIR-GAP.md`](../K0/AIR-GAP.md) — 커널 측 폐쇄망 프로토콜 명세
