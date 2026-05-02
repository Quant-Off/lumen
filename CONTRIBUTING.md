# 기여 가이드라인

이 프로젝트는 **보안 우선**이므로 PR은 Team Quant의 다른 프로젝트들과 동일하게 비교적 엄격한 기준을 따릅니다. 새로운 의존성은 정당화가 필요하여 이유와 audit 결과를 PR description에 명시해주셔야 하며, 가능하면 `default-features = false` 로 추가하고 필요한 feature만 활성화해주세요. `unsafe` 사용은 `lumen-sandbox` 만 허용되며 다른 곳에서는 `#![forbid(unsafe_code)]`가 강제됩니다. `lumen-sandbox`의 `unsafe`는 wasmtime FFI가 불가피하기 때문이고, 모든 `unsafe` 블록에 영문 `// SAFETY:` 주석이 필수입니다!

모든 PR 은 다음 네 가지 게이트를 통과해야 합니다.

- `cargo build --workspace --all-targets`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`

새로운 기능은 정확성과 보안 테스트를 동반해야 하며 적대적 입력 테스트가 포함되어야 합니다. 예를 들어, ONNX 디코더 PR에는 다음 부정적 테스트(negative-test)가 반드시 있어야 합니다.

- truncated varint
- deprecated group
- oversized length

**결정성을 깨는 변경은 명시적 정당화가 필요**하여 `f32` 사용, `HashMap` 사용, 시스템 시각 의존 등은 PR description에 명시되고 결정론 경로 영향이 없음이 증명되어야 합니다. 보안 영향이 있는 PR은 위협 모델 업데이트를 동반하여 새로운 공격면을 열거나 닫는 변경이면 위 *위협 모델* 절을 함께 갱신해야 하며, 마지막으로 취약점 보고는 비공개로 **GitHub issue 가 아닌 maintainer 이메일로 직접 보고 (책임감 있는 공개)** 하기를 요청합니다.

# 토의점

저희는 로드맵 마일스톤의 과정에서 암호학적 기능 제공자로 기본 [ELIB-K0-NT](https://github.com/Quant-Off/elib-k0-nt) 라이브러리 설정 후, 모듈식 또는 플래그를 통한 암호학 기능 추가(조립) 또는 변경하는 의견에 대해 어떻게 생각하시나요? ELIB-K0-NT는 암호학적 인증이 몇 부분 필요한 점이 있지만, 상수-시간 또는 소거 부문에선 확실함을 보이기도 해서 계속 고민 중에 있습니다.

# 라이선스

이 프로젝트는 Apache License 2.0 OR MIT License 듀얼 라이선스이며 사용자는 둘 중 어느 쪽이든 선택할 수 있습니다. 자세한 내용은 [LICENSE-APACHE](LICENSE-APACHE) 와 [LICENSE-MIT](LICENSE-MIT) 를 참고하세요. 이 라이선스 선택은 Rust 생태계의 표준 관행을 따른 것이며 상업·연구·정부 사용 모두 명시적으로 허용합니다 - 보안 인프라가 광범위하게 채택되려면 라이선스가 장벽이 되어서는 안 된다는 판단입니다.