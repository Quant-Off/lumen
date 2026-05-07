#!/usr/bin/env bash
#
# Lumen 폐쇄형(Air-Gapped) 환경 의존성 벤더링 스크립트.
#
# 동작:
#   1. cargo vendor 로 모든 워크스페이스 의존성 소스를 vendor/ 에 복제
#   2. .cargo/vendor-config.toml 을 .cargo/config.toml 로 활성화
#   3. 폐쇄망에서 필요한 시스템 바이너리 (protoc 등) 위치 안내
#
# 사용:
#   ./scripts/vendor.sh                # 기본 (default features)
#   ./scripts/vendor.sh --all          # candle/llama-cpp/crypto-channel 포함
#   ./scripts/vendor.sh --check        # vendor/ 무결성 재검증 (재실행 안전)
#
# 산출물:
#   vendor/                  ~500MB–1GB, .gitignore 됨
#   .cargo/config.toml       소스 교체 + net.offline=true
#
# 폐쇄망 이전 절차:
#   1. (온라인) ./scripts/vendor.sh --all
#   2. tar czf lumen-airgap-bundle.tar.gz . --exclude=target --exclude=.git
#   3. (오프라인) tar xzf lumen-airgap-bundle.tar.gz && cargo build --offline ...
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

MODE="default"
if [[ "${1:-}" == "--all" ]]; then
    MODE="all"
elif [[ "${1:-}" == "--check" ]]; then
    MODE="check"
elif [[ -n "${1:-}" ]]; then
    echo "사용법: $0 [--all | --check]" >&2
    exit 1
fi

echo "==> Lumen 의존성 벤더링 (mode=${MODE})"

# 1. 사전 점검: cargo, git
command -v cargo >/dev/null 2>&1 || { echo "cargo 가 PATH 에 없습니다." >&2; exit 1; }

# 2. cargo vendor 실행
VENDOR_ARGS=()
if [[ "$MODE" == "all" ]]; then
    # 모든 optional feature 의 의존성도 함께 가져옵니다.
    # candle 은 protoc 사전 설치 필요. 이 스크립트는 vendor 만 수행하고
    # 빌드는 별도이므로 protoc 부재여도 vendor 는 성공합니다.
    VENDOR_ARGS+=(
        --sync crates/lumen-channel/Cargo.toml
        --sync crates/lumen-inference/Cargo.toml
    )
fi

if [[ "$MODE" == "check" ]]; then
    if [[ ! -d vendor ]]; then
        echo "vendor/ 디렉토리가 없습니다. 먼저 $0 또는 $0 --all 을 실행하세요." >&2
        exit 1
    fi
    echo "==> cargo vendor 무결성 재검증 (--no-delete)"
    cargo vendor --no-delete vendor >/dev/null
else
    echo "==> cargo vendor 실행 — 약 500MB~1GB 의 소스를 vendor/ 로 가져옵니다..."
    cargo vendor "${VENDOR_ARGS[@]}" vendor >/dev/null
fi

# 3. .cargo/config.toml 활성화 (이미 존재하면 백업)
if [[ -f .cargo/config.toml && ! -L .cargo/config.toml ]]; then
    cp .cargo/config.toml ".cargo/config.toml.backup-$(date +%Y%m%d-%H%M%S)"
    echo "==> 기존 .cargo/config.toml 을 백업했습니다."
fi

cp .cargo/vendor-config.toml .cargo/config.toml
echo "==> .cargo/config.toml 활성화 (vendor-config.toml 복사)"

# 4. 결과 요약
VENDOR_SIZE=$(du -sh vendor 2>/dev/null | cut -f1)
VENDOR_COUNT=$(find vendor -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
echo ""
echo "==> 완료"
echo "    vendor/ 크기      : ${VENDOR_SIZE}"
echo "    벤더된 크레이트   : ${VENDOR_COUNT} 개"
echo ""
echo "다음 단계 (4-게이트):"
echo "    cargo build  --offline --workspace --all-targets"
echo "    cargo test   --offline --workspace"
echo "    cargo clippy --offline --workspace --all-targets -- -D warnings"
echo "    cargo fmt    --all -- --check"
echo ""
echo "옵셔널 feature 빌드:"
echo "    cargo build --offline --workspace --features lumen-channel/crypto-channel,lumen-inference/llama-cpp"
echo ""
echo "polleur 폐쇄망 시스템 바이너리 (워크스페이스 외부):"
echo "    - protoc (lumen-inference/candle feature 활성화 시 필수)"
echo "    - cmake  (lumen-inference/llama-cpp feature 활성화 시 필수, v0.5)"
echo ""
echo "AIR-GAPPED.md 참고."
