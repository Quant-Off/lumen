# Lumen

[![Language](https://img.shields.io/badge/README-English_Ver-blue?style=for-the-badge)](README.md)
[![Lumen-Ver](https://img.shields.io/badge/Lumen_Milestone-v0.5.0-000000?style=for-the-badge)](https://github.com/Quant-Off/)
[![Qu4nt-Space-Discord](https://img.shields.io/badge/Qu4nt_Space-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/9utg4hp3m8)

Lumen 은 제로 트러스트(Zero-Trust) 와 폐쇄(Air-Gapped) 환경을 위한 고보안, 검증 가능한 Rust AI 에이전트 프레임워크입니다. 권한 통제(WASM 샌드박스) 와 결과 검증(zkML) 을 결합하여 국가기관 수준의 엄격한 보안 규격을 충족하는 안전한 자율형 AI 구동을 목표로 합니다.

모든 모델, 정책 파일, 추론 엔진 바이너리는 사용 전에 BLAKE3 해시로 핀됩니다. 에이전트는 wasmtime 샌드박스 안에서 실행되고 Ed25519 서명된 capability 를 통해서만 도구에 도달합니다. 도구 라우팅 결정은 이후 검증 가능한 commitment 에 바인드됩니다. 모든 외부 소스는 vendor 로 고정되어 워크스페이스 전체가 오프라인에서 빌드됩니다.

## 문서

- [USAGE_KR.md](USAGE_KR.md): 빠른 시작, CLI 서브커맨드, 워크스페이스 구조, feature 플래그, 검증 게이트, 현재 한계
- [INTRODUCTION_KR.md](INTRODUCTION_KR.md): 설계 철학, 4개 보안 축, 위협 모델, 로드맵
- [PRACTICE_LLM_KR.md](PRACTICE_LLM_KR.md): llama.cpp `llama-server` 백엔드로 Lumen 위에서 실제 LLM 구동하기
- [INFERENCE_KR.md](INFERENCE_KR.md): 추론 엔진 선정, 신뢰 경계, 다른 엔진 추가 방법

## 빠른 시작

```bash
$ cargo build --workspace
$ cargo test  --workspace
$ cargo run -p lumen-cli --example hello_agent
```

예제는 echo, add, jailbreak 차단을 에이전트 3스텝으로 보여줍니다. 전체 CLI 는 [USAGE_KR.md](USAGE_KR.md) 를 참고하세요.

## 라이선스

Apache-2.0 OR MIT 듀얼 라이선스이며 사용자가 둘 중 하나를 자유롭게 선택할 수 있습니다. 자세한 내용은 [LICENSE-APACHE](LICENSE-APACHE) 와 [LICENSE-MIT](LICENSE-MIT) 를 참고하세요.

## 기여

Lumen 은 AI 와 보안 오픈소스 생태계 기여를 목적으로 합니다.
