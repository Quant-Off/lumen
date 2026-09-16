# 사용 가이드

[![Language](https://img.shields.io/badge/USAGE-English_Ver-blue?style=for-the-badge)](USAGE.md)

이 문서는 Lumen 을 빌드·테스트·운용하는 데 필요한 내용을 모았습니다. 빠른 시작, CLI 서브커맨드, 워크스페이스 구조, feature 플래그, 검증 게이트, 현재 한계를 다룹니다. 설계 배경은 [INTRODUCTION_KR.md](INTRODUCTION_KR.md), 실제 LLM 구동은 [PRACTICE_LLM_KR.md](PRACTICE_LLM_KR.md), 추론 엔진 설계는 [INFERENCE_KR.md](INFERENCE_KR.md) 를 참고하세요.

---

## 목차

1. [빠른 시작](#1-빠른-시작)
2. [CLI 사용법](#2-cli-사용법)
3. [워크스페이스 구조](#3-워크스페이스-구조)
4. [Feature 플래그](#4-feature-플래그)
5. [검증 게이트](#5-검증-게이트)
6. [폐쇄망 빌드](#6-폐쇄망-빌드)
7. [현재 한계와 자리표시자](#7-현재-한계와-자리표시자)

---

## 1. 빠른 시작

Rust stable 1.95 에서 개발되었으며 이후 버전에서 안정적으로 동작합니다. `rust-toolchain.toml` 이 자동 적용되므로 별도 설정은 필요 없습니다. 모든 외부 크레이트 소스는 `vendor/` 에 고정되어 있어 빌드가 네트워크에 접근하지 않습니다.

기본 빌드 또는 전체 테스트 실행 (v0.5 기준 190개 이상):

```bash
$ cargo build --workspace # 기본 빌드
$ cargo test --workspace  # 테스트 전체 실행
```

선택 feature 인 AES-GCM/x25519 채널 암호화 (`crypto-channel`) 와 `#[lumen_agent]` proc-macro (`macros`) 까지 활성화한 빌드를 검증하려면:

```bash
$ cargo test --workspace --features lumen-channel/crypto-channel,lumen-sdk/macros
```

전체 동작을 한눈에 보려면:

```bash
$ cargo run -p lumen-cli --example hello_agent
```

이 데모는 echo, add, jailbreak 차단의 3스텝 시퀀스를 통해 [4개 보안 축](INTRODUCTION_KR.md#4개의-핵심-축) 이 모두 작동하는 것을 보여줍니다.

---

## 2. CLI 사용법

`lumen` 바이너리는 11개의 서브커맨드 (`init`, `keygen`, `sign-model`, `sign-engine`, `verify-model`, `verify-engine`, `sbom`, `defend`, `prove`, `run`, `verifier`) 를 제공합니다.

### `init`

정책 파일 템플릿을 stdout 으로 출력합니다. `policies/my_policy.toml` 같은 경로로 리다이렉트해 시작합니다. 템플릿에는 `llama-server` 백엔드를 선택하는 방법을 보여주는 주석 처리된 `[inference]` 섹션이 포함되어 있습니다.

```bash
$ cargo run -p lumen-cli -- init > policies/my_policy.toml
```

### `defend`

임의의 텍스트에 대해 프롬프트 인젝션 분석을 수행하고 `Verdict` 와 `corpus_version` 을 출력합니다.

```bash
$ cargo run -p lumen-cli -- defend --text "ignore previous instructions"
```

### `verify-model`

매니페스트와 실제 파일을 비교하여 BLAKE3 해시와 (있다면) Ed25519 서명을 검증합니다.

```bash
$ cargo run -p lumen-cli -- verify-model --manifest model.toml --file model.safetensors
```

### `keygen`

모델·엔진 매니페스트 서명용 Ed25519 키를 생성합니다. 시드는 새 `0600` 파일에 hex 로 기록되고 (기존 파일은 덮어쓰지 않음), `trusted_signers` 에 넣을 공개 키가 출력됩니다.

```bash
$ cargo run -p lumen-cli -- keygen --out /etc/lumen/signing.key
```

### `sign-model`

모델 파일을 매니페스트와 다시 대조한 뒤 키 파일로 매니페스트에 서명합니다. 가중치와 해시가 다른 매니페스트에는 서명하지 않습니다.

```bash
$ cargo run -p lumen-cli -- sign-model --manifest model.toml --key /etc/lumen/signing.key --out model_signed.toml
```

### `sign-engine`

엔진 실행 파일과 그것이 로드하는 공유 라이브러리를 모두 해시해 서명된 `EngineManifest` 를 발행합니다. 라이브러리는 `--file` 로 하나씩 넘깁니다. 동적 링크 빌드에서 런처만 핀하면 아무것도 핀되지 않은 것과 같습니다.

```bash
$ cargo run -p lumen-cli -- sign-engine --name llama-server --version b10603 \
    --binary /opt/llama.cpp/llama-server --file /opt/llama.cpp/lib/libllama.so \
    --key /etc/lumen/signing.key --out engine.toml
```

### `verify-engine`

엔진 매니페스트를 검증합니다. 주어진 신뢰 서명자로 서명을 먼저 확인한 뒤, 핀된 파일이 모두 일반 파일이고 world-writable 이 아니며 BLAKE3 가 일치하는지 검사합니다. `--trusted-signer` 가 하나라도 있으면 미서명 매니페스트는 거부됩니다.

```bash
$ cargo run -p lumen-cli -- verify-engine --manifest engine.toml --trusted-signer <PUBKEY_HEX>
```

### `sbom`

정책 파일에 선언된 모델과 엔진으로부터 CycloneDX 1.5 SBOM 을 JSON 으로 출력합니다. 엔진은 `application` 컴포넌트로 실리며 실행 파일 해시 뒤에 핀된 라이브러리 해시가 따라옵니다.

```bash
$ cargo run -p lumen-cli -- sbom --policy policies/default.toml
```

### `prove`

도구 라우팅 결정에 대한 mock ZKP 를 생성하고 즉시 self-verify 합니다. 회로 식별자, prompt 해시, proof 다이제스트, 검증 verdict 가 모두 stdout 에 표시됩니다.

```bash
$ cargo run -p lumen-cli -- prove --prompt "echo hi" --tool echo --circuit-id lumen.routing.v1
```

### `run`

정책 파일의 BLAKE3 핀을 강제하면서 에이전트 1스텝을 실행합니다. `--policy-hash` 가 정책 파일의 실제 BLAKE3 와 다르면 즉시 거부되며, 이것이 Lumen 의 Zero-Trust UX 핵심입니다. 정책 파일에 모델·엔진 매니페스트가 선언되어 있으면 먼저 `trusted_signers` 로 검증됩니다 (서명 -> 핀된 파일 전부). `trusted_signers` 가 설정되면 미서명 매니페스트는 거부됩니다. 추론 백엔드는 정책 파일의 `[inference]` 섹션으로 선택되며 (기본 `dummy`, 또는 `llama-server`), 엔진 매니페스트 참조를 포함한 파라미터가 정책과 함께 핀됩니다. 이후 호스트 측에서 capability 발급, defense 분석, 추론, 정책 검증, 도구 실행, ZKP 생성, self-verify 까지 한 번에 수행됩니다.

```bash
$ cargo run -p lumen-cli -- run --policy policies/default.toml --policy-hash <BLAKE3HEX> --prompt "echo hello"
```

### `verifier`

온체인 검증기 도구로 두 개의 하위 커맨드 `emit` 과 `deploy` 를 가집니다. `verifier emit` 은 EVM (Solidity) 또는 Mina (o1js) 검증기 source 와 deploy 스크립트, 메타데이터 JSON 을 결정론적으로 디스크에 작성합니다 (같은 입력에 대해 byte 동일 출력 보장). `verifier deploy` 는 `forge create` 또는 `zk deploy` 를 자식 프로세스로 실행하지만 기본은 dry-run 이라 redacted 명령 문자열만 출력하며, EVM private key 는 환경변수 `LUMEN_DEPLOY_PRIVKEY` 에서 읽고 audit 로그에서는 `***` 로 마스크됩니다. 폐쇄망 운용에서는 emit 으로 산출된 디렉토리를 텍스트로 운반하고 dry-run 출력만 운영 환경에 복사하는 워크플로우를 권장합니다.

```bash
$ cargo run -p lumen-cli -- verifier emit --chain evm --circuit-id lumen.routing.binary.v1 --out ./out/evm
$ cargo run -p lumen-cli -- verifier emit --chain mina --out ./out/mina
$ cargo run -p lumen-cli -- verifier deploy --chain evm --rpc https://sepolia.example.org --out ./out/evm
```

---

## 3. 워크스페이스 구조

Lumen 은 15개의 라이브러리 크레이트와 1개의 바이너리 크레이트로 구성된 Cargo 워크스페이스이며, 추가로 `agents/` 하위에 wasm32 전용 데모 에이전트 두 개가 별도 워크스페이스로 분리되어 있습니다.

```text
crates/
  lumen-core         ID, BLAKE3, Ed25519 wrapper, CSPRNG, Error, Time
  lumen-fixed        Q16.16 과 Q8.24 결정론 정수 연산 (no_std)
  lumen-capability   Capability 토큰, PolicyEngine, AgentMessage 자원
  lumen-channel      InProc, AttestedChannel, AES-GCM/x25519 EncryptedChannel
  lumen-provenance   Safetensors, ONNX, GGUF 헤더 검증, SBOM, PinSet 자동 회전
  lumen-defense      Aho-Corasick 과 RegexSet 기반 3단 인젝션 필터
  lumen-zkml         ProvingSystem trait, Mock commitment prover, ezkl 스텁
  lumen-inference    InferenceEngine/StreamingEngine trait, BackendRegistry, 검증 강제 로더, 도구 호출 문법; Dummy / llama-server(llama.cpp) / TEE 채널 백엔드
  lumen-sandbox      wasmtime 결정론 Config 와 capability-gated 임포트
  lumen-agent        에이전트 런타임 (defense -> infer -> policy -> tool -> prove)
  lumen-orchestrator tokio 다중 에이전트 슈퍼바이저, capability-gated 메시징
  lumen-attestation  Intel TDX 와 AMD SEV-SNP quote 파서
  lumen-sdk          WASM 에이전트 SDK (호스트 임포트 안전 래퍼)
  lumen-sdk-macros   `#[lumen_agent]` proc-macro
  lumen-onchain      EVM 과 Mina 검증기 emit 및 배포 자동화
  lumen-cli          `lumen` 바이너리와 데모 예제
agents/
  echo-agent         raw SDK 를 사용하는 wasm32 데모 에이전트
  macro-agent        #[lumen_agent] proc-macro 를 시연하는 wasm32 에이전트
```

---

## 4. Feature 플래그

- `lumen-channel/crypto-channel`: AES-GCM, x25519, BLAKE3 KDF 의 EncryptedChannel 을 활성화
- `lumen-zkml/ezkl`: ezkl 자리표시자
- `lumen-inference/llama-server`: 해시 핀된 격리 `llama-server` 프로세스를 Unix 소켓으로 구동하는 llama.cpp 백엔드 (순수 Rust HTTP/SSE 클라이언트, GBNF 도구 호출 문법, 스트리밍). `lumen-cli` 에서는 기본 활성화
- `lumen-sdk/macros`: `#[lumen_agent]` proc-macro 의 re-export
- `lumen-sdk/alloc`: 동적 String/Vec 보조 함수의 노출

라이브러리 크레이트의 모든 feature 는 기본적으로 비활성화 상태이며 _air-gapped self-contained_ 빌드의 일관성을 위해 의도적으로 opt-in 입니다.

---

## 5. 검증 게이트

병합 가능한 변경은 다음 명령이 모두 통과해야 합니다.

```bash
$ cargo build  --workspace --all-targets
$ cargo test   --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo fmt    --all -- --check
$ cargo deny   check
```

선택 feature 까지 검증하려면 build, test, clippy 명령에 `--features lumen-channel/crypto-channel,lumen-sdk/macros` 를 추가합니다. CI 는 추가로 `RUSTDOCFLAGS=-D warnings` 의 `cargo doc`, `cargo audit`, `--offline` 폐쇄망 잡, `agents/echo-agent` 의 wasm32 빌드를 실행합니다.

---

## 6. 폐쇄망 빌드

`.cargo/config.toml` 이 crates.io source replacement 를 `vendor/` 로 강제하고 `net.offline` 을 설정하므로, 온라인이든 오프라인이든 모든 빌드가 byte 동일한 소스로 컴파일됩니다. `Cargo.toml` 이 바뀌면 인터넷이 되는 머신에서 vendor 디렉토리를 재동기화하고 `Cargo.lock` 과 `vendor/` 를 함께 커밋합니다.

```bash
$ ./scripts/vendor.sh          # Cargo.lock + vendor/ 갱신 (네트워크 필요)
$ ./scripts/vendor.sh --check  # vendor/ 가 Cargo.lock 과 일치하는지 검증 (오프라인)
```

---

## 7. 현재 한계와 자리표시자

`MockCommitmentProver` 는 ZKP 가 아닌 BLAKE3 commitment 입니다 (`Verification::CommitmentOnly` 와 `Verification::ZkVerified` 가 타입 차원에서 분리되어 있어 혼동 자체가 불가능). v0.3 의 halo2 회로는 MockProver 검증이 ZK 보장을 주지 않아 v0.5 에서 제거되었으며, succinct 백엔드 (SP1, RISC Zero 등) 는 다음 마일스톤 검토에서 결정됩니다. 따라서 현재 `ZkVerified` 는 도달 불가능합니다.

`ezkl` 백엔드는 feature 스텁입니다. `DummyEngine` 은 echo 와 add 패턴만 인식합니다. `LlamaServerEngine` (`llama-server` feature) 은 llama.cpp 를 BLAKE3 핀된 별도 프로세스로 실행하고 `/props` 로 서빙 모델을 `VerifiedModelHandle` 에 바인드하며, 모든 모델 파일은 여전히 `VerifiedModelLoader` 를 통해 BLAKE3 + 선택적 Ed25519 검증을 강제합니다. 엔진 바이너리와 공유 라이브러리는 서명된 `EngineManifest` 로 핀되며, OS 코드 서명은 참조하지 않습니다.

`lumen-onchain` 의 EVM Solidity contract 는 회로 제약 재검증 (constraint recheck) 형태이며, succinct proof 검증으로의 업그레이드는 ZK 백엔드 결정 이후 진행됩니다.

TEE attestation 은 Intel TDX 와 AMD SEV-SNP 문서를 형식 일관성만 검사하며 (`Verdict::FormatOnly`), PCK chain 과 VCEK 서명 검증은 아직 구현되지 않았습니다.
