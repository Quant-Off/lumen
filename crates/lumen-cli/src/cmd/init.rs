//! `lumen init` - 샘플 정책 TOML 을 stdout 으로 출력.

use clap::Args as ClapArgs;

/// `init` 인자.
#[derive(Debug, ClapArgs)]
pub struct Args {}

/// 실행.
pub fn run(_args: Args) -> anyhow::Result<()> {
    print!(
        r##"# Lumen 샘플 정책 파일.
#
# 배포 전에 placeholder hex 값을 실제 키로 교체하세요.
# `cargo run -p lumen-cli -- run --policy <PATH> --policy-hash <BLAKE3HEX> --prompt "echo hi"`
# 는 <PATH> 의 BLAKE3 가 <BLAKE3HEX> 와 다르면 실행을 거부합니다.

[agent]
id = "0102030405060708090a0b0c0d0e0f10"

# 아래 capability 서명을 검증할 키. hex 인코딩된 Ed25519 공개키 사용
# (32 바이트 = 64 hex 자).
trusted_issuers = []

# Capability - 부트스트랩 파일에는 비워 둡니다. 데모 예제는 프로그램적으로
# 작성합니다 - `examples/hello_agent.rs` 참고.
capabilities = []

# 모델·엔진 매니페스트 서명을 검증할 신뢰 서명자 (`lumen keygen` 의 공개 키).
# 하나라도 넣으면 미서명 매니페스트는 거부됩니다.
trusted_signers = []

# 모델 매니페스트. Lumen 은 엔진이 바이트에 닿기 전에 BLAKE3 + (옵션)
# Ed25519 서명을 검증합니다. `lumen sign-model` 로 signature / signer 를 채웁니다.
[[models]]
name = "tiny-demo"
version = "0.0.1"
path = "models/tiny.safetensors"
format = "Safetensors"
hash = "0000000000000000000000000000000000000000000000000000000000000000"
license = "Apache-2.0"

# 추론 백엔드. 생략하면 결정론적 `dummy` 엔진 (echo / add 패턴만 인식).
# llama.cpp 를 쓰려면 아래 예시처럼 `llama-server` 를 선택합니다. 엔진
# 바이너리와 모델의 BLAKE3 가 이 파일과 함께 핀됩니다 (INFERENCE.md 참고).
[inference]
backend = "dummy"

# 엔진 매니페스트. `lumen sign-engine` 출력을 그대로 붙여 넣습니다. 실행 파일과
# 공유 라이브러리 전부가 핀되고 서명은 `trusted_signers` 로 검증됩니다.
# [[engines]]
# name      = "llama-server"
# version   = "b10603"
# path      = "/opt/llama.cpp/llama-server"
# hash      = "<실행 파일의 BLAKE3 hex>"
# files     = [{{ path = "/opt/llama.cpp/lib/libllama.so", hash = "<BLAKE3 hex>" }}]
# signature = "<hex>"
# signer    = "<서명자 공개 키 hex>"

# [inference]
# backend = "llama-server"
# [inference.params]
# mode        = "spawn"                      # 또는 "attach"
# endpoint    = "unix:/run/lumen/llama.sock" # 또는 "tcp:127.0.0.1:8080"
# binary      = "llama-server"               # [[engines]] 의 name 또는 경로 + binary_hash
# model       = "tiny-demo"                  # [[models]] 의 name 또는 GGUF 경로
# n_ctx       = "4096"
# gpu_layers  = "99"
# parallel    = "1"                          # 결정론 필요 시 1
"##
    );
    Ok(())
}
