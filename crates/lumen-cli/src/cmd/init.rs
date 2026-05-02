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

# 모델 매니페스트. Lumen 은 엔진이 바이트에 닿기 전에 BLAKE3 + (옵션)
# Ed25519 서명을 검증합니다.
[[models]]
name = "tiny-demo"
version = "0.0.1"
path = "models/tiny.safetensors"
format = "Safetensors"
hash = "0000000000000000000000000000000000000000000000000000000000000000"
license = "Apache-2.0"
"##
    );
    Ok(())
}
