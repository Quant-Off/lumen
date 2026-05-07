# Lumen 프로젝트 목표

제로 트러스트(Zero-Trust) 및 폐쇄(Air-Gapped) 환경을 위한 고보안, 검증 가능한 Rust 기반 AI 에이전트 프레임워크. 권한 통제(WASM 샌드박스)와 결과 검증(zkML)을 결합하여, 국가기관 수준의 엄격한 보안 규격을 충족하는 안전한 자율형 AI 구동을 목표.

**AI 및 보안 오픈소스 생태계 기여를 위한 프로젝트.**

구현 필요 기능 다음과 같음:

1. 하이브리드 zkML 파이프라인
   - 선택적 ZK 증명: 연산 비용이 높은 LLM 전체 텍스트 생성 대신, 에이전트의 '도구 선택(Tool-use routing)' 및 '출력 필터링/정책 준수' 로직에만 ezkl 또는 ZKTorch를 적용하여 ZK Proof 생성.
   - 자동화 및 검증: CI/CD 파이프라인에서 Proof를 자동 생성하고, 온체인(Mina/EVM) 또는 오프체인 검증기(Verifier)에 배포 및 확인하는 모듈.
2. 권한 분리형 Host-WASM 샌드박스
   - WASM 격리 (Agent Logic): wasmtime과 Rust의 소유권 모델을 활용해 에이전트의 도구 실행 및 통신 로직을 완벽히 격리. 파일, 네트워크, 시스템 환경 접근은 명시적인 Capability 기반으로만 최소한으로 허용.
   - Host TEE 실행 (LLM Inference): 하드웨어 가속(GPU)을 온전히 활용하기 위해 무거운 모델 추론은 호스트의 신뢰 실행 환경(TEE) 내부에서 실행하고, WASM 샌드박스와는 격리된 보안 채널(Secure Channel)로만 통신.
3. 결정론적(Deterministic) 제어 모듈
   - 상태 격리: 멀티 에이전트 오케스트레이션 시 메모리 공유 없이 tokio 기반 비동기 처리로 상태 전이의 안정성 확보.
   - 연산 통제: ZK 증명의 신뢰성을 보장하기 위해 부동소수점 연산의 하드웨어 의존성을 배제하는 모델 양자화(Quantization) 및 고정소수점(Fixed-point) 연산 파이프라인 구축.
4. 고속 보안 방어 및 출처 검증 엔진
   - 실시간 위협 탐지: Rust의 성능을 살려 지연 시간(Latency)을 최소화한 프롬프트 인젝션 및 제일브레이크 방어/필터링 모듈 탑재.
   - 모델 Provenance (공급망 보안): ONNX, Safetensors 모델 로드 시 파일 해시 및 서명을 즉각 검증하고, SBOM을 강제 생성하여 승인되지 않은 모델 가중치 변조를 원천 차단.

# 구현에 앞서

모든 설계는 보안성 우선. 단, 연산 비용 (작업 속도) 극강 최적화 필요.

Docstring 한국어로 작성.

마일스톤 `v1.0`(정부 또는 규제 환경에서 production 배포, [FIPS 140-3 compliance audit](https://csrc.nist.gov/pubs/fips/140-3/final), [Kani](https://www.in-com.com/ko/blog/the-rust-developers-toolbox-best-static-code-analysis-tools/#Kani) 또는 [Prusti](https://github.com/viperproject/prusti-dev) 등을 활용한 일부 모듈의 형식 검증, 외부 보안 audit 1회 통과)은 당분간 진행 X.

# 기능 수정 또는 추가 시

`Cargo.toml`이 변경된 경우 `.lock` 파일 갱신하기.

프로젝트는 항상 다음 네 개의 명령어를 모두 통과해야 함.

```bash
$ cargo build  --workspace --all-targets
$ cargo test   --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo fmt    --all -- --check
```