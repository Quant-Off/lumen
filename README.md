# Lumen

[![Language](https://img.shields.io/badge/README-English_Ver-blue?style=for-the-badge)](README_EN.md)
[![Lumen-Ver](https://img.shields.io/badge/Lumen_Milestone-v0.4-000000?style=for-the-badge)](https://github.com/Quant-Off/)
[![Qu4nt-Space-Discord](https://img.shields.io/badge/Qu4nt_Space-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://github.com/Quant-Off/)

Lumen은 제로 트러스트(Zero-Trust)와 폐쇄(Air-Gapped) 환경을 위한 고보안, 검증 가능한 Rust AI 에이전트 프레임워크입니다. 권한 통제(WASM 샌드박스)와 결과 검증(zkML)을 결합하여 국가기관 수준의 엄격한 보안 규격을 충족하는 안전한 자율형 AI 구동을 목표로 합니다. 자세한 설계 철학과 기술적 배경은 [INTRODUCTION.md](INTRODUCTION.md) 문서를 참고하세요.

## 빠른 시작

Rust stable 1.82에서 개발되었으며 이후 버전에 대해 안정적으로 동작합니다. `rust-toolchain.toml`이 자동 적용되므로 별도 설정은 필요 없습니다.

기본 빌드 또는 [v0.4](INTRODUCTION.md#로드맵) 기준 140개 이상 테스트 전체 실행은 다음을 수행하세요.

```bash
$ cargo build --workspace # 기본 빌드
$ cargo test --workspace # 테스트 전체 실행
```

AES-GCM/x25519 채널 암호화(`crypto-channel`), halo2 라우팅 회로 (`halo2`), `#[lumen_agent]` proc-macro (`macros`)등 모든 feature를 활성화한 빌드까지 검증하려면 다음을 수행하세요.

```bash
$ cargo test --workspace --features lumen-channel/crypto-channel,lumen-zkml/halo2,lumen-sdk/macros
```

전체 동작을 한 눈에 보려면 다음을 수행하세요.

```bash
$ cargo run -p lumen-cli --example hello_agent
```

이 데모는 echo, add, jailbreak 차단의 3스텝 시퀀스를 통해 [4개 보안 축](INTRODUCTION.md#4개의-핵심-축)이 모두 작동하는 것을 보여줍니다.

## CLI 사용법

`lumen` 바이너리는 v0.4 기준 7개의 서브커맨드 (`init`, `verify-model`, `sbom`, `defend`, `prove`, `run`, `verifier`) 를 제공합니다. `init`은 정책 파일 템플릿을 stdout 으로 출력하므로 `policies/my_policy.toml` 같은 경로로 리다이렉트해 사용을 시작합니다.

```bash
$ cargo run -p lumen-cli -- init > policies/my_policy.toml
```

`defend`는 임의의 텍스트에 대해 프롬프트 인젝션 분석을 수행하고 `Verdict`와 `corpus_version`을 출력합니다.

```bash
$ cargo run -p lumen-cli -- defend --text "ignore previous instructions"
```

`verify-model`은 매니페스트와 실제 파일을 비교하여 BLAKE3 해시와 (있다면) Ed25519 서명을 검증합니다.

```bash
$ cargo run -p lumen-cli -- verify-model --manifest model.toml --file model.safetensors
```

`sbom`은 정책 파일에 선언된 모델 목록으로부터 CycloneDX 1.5 SBOM을 JSON으로 출력합니다.

```bash
$ cargo run -p lumen-cli -- sbom --policy policies/default.toml
```

`prove`는 도구 라우팅 결정에 대한 mock ZKP를 생성하고 즉시 self-verify합니다. 회로 식별자, prompt 해시, proof 다이제스트, 검증 verdict가 모두 stdout에 표시됩니다.

```bash
$ cargo run -p lumen-cli -- prove --prompt "echo hi" --tool echo --circuit-id lumen.routing.v1
```

`run`은 정책 파일의 BLAKE3 핀을 강제하면서 에이전트 1스텝을 실행합니다. `--policy-hash`가 정책 파일의 실제 BLAKE3와 다르면 즉시 거부되며, 이것이 Lumen의 Zero-Trust UX 핵심입니다. 정책 파일 안에 선언된 모델 매니페스트가 있으면 `verify_model`단계가 자동으로 통과되어야 하며, 통과 후에는 호스트 측에서 capability 발급, defense 분석, mock 추론, 정책 검증, 도구 실행, ZKP 생성, self-verify까지 한 번에 수행됩니다.

```bash
$ cargo run -p lumen-cli -- run --policy policies/default.toml --policy-hash <BLAKE3HEX> --prompt "echo hello"
```

`verifier`는 v0.4 에 추가된 온체인 검증기 도구로 두 개의 하위 커맨드 `emit`과 `deploy`를 가집니다. `verifier emit`은 EVM(Solidity) 또는 Mina(o1js) 검증기 source 와 deploy 스크립트, 메타데이터 JSON 을 결정론적으로 디스크에 작성합니다(같은 입력에 대해 byte-동일한 출력 보장). `verifier deploy`는 `forge create` 또는 `zk deploy`를 자식 프로세스로 실행하지만 기본은 dry-run 이라 redacted 명령 문자열만 출력하며, EVM private key는 환경변수 `LUMEN_DEPLOY_PRIVKEY`에서 읽고 audit 로그에서는 `***`로 마스크되어 누출이 차단됩니다. 에어갭(폐쇄환경) 운용에서는 emit으로 산출된 디렉토리를 텍스트로 운반하고 dry-run 출력만 운영 환경에 복사하는 워크플로우가 권장됩니다.

```bash
$ cargo run -p lumen-cli -- verifier emit --chain evm --circuit-id lumen.routing.binary.v1 --out ./out/evm
$ cargo run -p lumen-cli -- verifier emit --chain mina --out ./out/mina
$ cargo run -p lumen-cli -- verifier deploy --chain evm --rpc https://sepolia.example.org --out ./out/evm
```

## 워크스페이스 구조

Lumen은 v0.4 기준 15개의 라이브러리 크레이트와 1개의 바이너리 크레이트로 구성된 Cargo 워크스페이스이며, 추가로 `agents/` 하위에 wasm32 전용 데모 에이전트 두 개가 별도 워크스페이스로 분리되어 있습니다.

```text
crates/
  lumen-core         ID, BLAKE3, Ed25519 wrapper, Error, Time
  lumen-fixed        Q16.16와 Q8.24 결정론 정수 연산 (no_std)
  lumen-capability   Capability 토큰, PolicyEngine, AgentMessage 자원
  lumen-channel      InProc, AttestedChannel, AES-GCM/x25519 EncryptedChannel
  lumen-provenance   Safetensors, ONNX, GGUF 헤더 검증, SBOM, PinSet 자동 회전
  lumen-defense      Aho-Corasick와 RegexSet 기반 3단 인젝션 필터
  lumen-zkml         ProvingSystem trait, Mock, ezkl 스텁, halo2 회로
  lumen-inference    InferenceEngine/StreamingEngine trait, 검증 강제 로더, 양자화 설정; Dummy/CandleLlm(GGUF)/TEE 채널 백엔드
  lumen-sandbox      wasmtime 결정론 Config 와 capability-gated 임포트
  lumen-agent        에이전트 런타임 (defense, infer, policy, tool, prove 순)
  lumen-orchestrator tokio 다중 에이전트 슈퍼바이저, capability-gated 메시징
  lumen-attestation  Intel TDX 와 AMD SEV-SNP quote 파서
  lumen-sdk          WASM 에이전트 SDK (호스트 임포트 안전 래퍼)
  lumen-sdk-macros   `#[lumen_agent]` proc-macro
  lumen-onchain      EVM와 Mina 검증기 emit 및 배포 자동화
  lumen-cli          `lumen` 바이너리 와 데모 예제
agents/
  echo-agent         raw SDK를 사용하는 wasm32 데모 에이전트
  macro-agent        #[lumen_agent] proc-macro를 시연하는 wasm32 에이전트
```

## Feature 플래그

- `lumen-channel/crypto-channel`: AES-GCM, x25519, BLAKE3 KDF의 EncryptedChannel을 활성화
- `lumen-zkml/halo2`: halo2 binary argmax회로와 PLONKish제약을 컴파일 
- `lumen-zkml/ezkl`: ezkl 자리표시자
- `lumen-inference/candle-llm`: GGUF 양자화 LLM 추론(candle-transformers + HuggingFace 토크나이저, 토큰 단위 스트리밍 포함)
- `lumen-inference/llama-cpp`: llama.cpp 백엔드 인터페이스 스텁(v0.5 완성 예정, cmake 빌드 필요)
- `lumen-sdk/macros`: `#[lumen_agent]` proc-macro의 re-export
- `lumen-sdk/alloc`: 동적 String/Vec 보조 함수의 노출

모든 feature는 기본적으로 비활성화 상태이며 _air-gapped self-contained_ 빌드의 일관성을 위해 의도적으로 opt-in입니다.

## 검증 게이트

머지 가능한 변경은 다음 네 개의 명령이 모두 통과해야 합니다.

```bash
$ cargo build  --workspace --all-targets
$ cargo test   --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo fmt    --all -- --check
```

모든 v0.4 feature까지 검증하려면 위 두 번째 와 세 번째 명령에 `--features lumen-channel/crypto-channel,lumen-zkml/halo2,lumen-sdk/macros`를 추가합니다.

## 현재 한계와 자리표시자

`MockCommitmentProver`는 ZKP가 아닌 BLAKE3 commitment 입니다(`Verification::CommitmentOnly`와 `Verification::ZkVerified`가 타입 차원에서 분리되어 있어 혼동 자체가 불가능).

`Halo2Prover`는 실제 PLONKish 회로지만 v0.4 시점에서는 succinct KZG 백엔드 대신 MockProver로 검증하므로 off-line verifiability는 빠져 있고 후속 작업에서 KZG로 옮겨갑니다.

`ezkl` 백엔드는 feature 스텁입니다. `DummyEngine`은 echo와 add 패턴만 인식합니다. `CandleLlmEngine`(`candle-llm` feature)은 GGUF 양자화 모델을 로드해 토큰 단위 스트리밍 생성을 지원하며, 모든 모델 파일은 `VerifiedModelLoader`를 통해 BLAKE3 + 선택적 Ed25519 검증을 강제합니다. llama.cpp 백엔드(`llama-cpp` feature)는 인터페이스만 정의된 스텁이며 다음 마일스톤에서 완성될 예정입니다.

`lumen-onchain`의 EVM Solidity contract는 회로 제약 재검증(constraint recheck)형태이며 succinct proof 검증으로의 업그레이드는 halo2 KZG 마이그레이션과 동시에 진행될 예정입니다.

TEE attestation의 PCK chain와 VCEK 서명 검증은 v0.4 시점에서 measurement 핀 단계까지만 구현되어 있습니다.

## 라이선스

Apache-2.0 OR MIT 듀얼 라이선스이며 사용자가 둘 중 하나를 자유롭게 선택할 수 있습니다. 자세한 내용은 [LICENSE-APACHE](LICENSE-APACHE)와 [LICENSE-MIT](LICENSE-MIT) 를 참고하세요.

## 기여

Lumen은 AI 와 보안 오픈소스 생태계 기여를 목적으로 합니다. 보안 우선 프로젝트이므로 기여 가이드라인이 비교적 엄격하며 자세한 내용은 [CONTRIBUTING.md](CONTRIBUTING.md)의 기여 가이드라인을 참고하세요.