#!/usr/bin/env bash
#
# Lumen 외부 의존성 벤더링 스크립트.
#
# 모든 외부 크레이트 소스는 vendor/ 에 고정되어 저장소와 함께 커밋됩니다
# (.cargo/config.toml 의 source replacement + net.offline=true).
# 이 스크립트는 Cargo.toml 변경 후 vendor/ 를 재동기화할 때만 실행합니다.
#
# 사용:
#   ./scripts/vendor.sh            # Cargo.lock 갱신 + vendor/ 재동기화 (온라인 필요)
#   ./scripts/vendor.sh --check    # vendor/ 가 Cargo.lock 과 일치하는지 오프라인 검증
#
# 산출물:
#   vendor/                  약 170MB, 커밋 대상
#   Cargo.lock               갱신 시 함께 커밋
#

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

MODE="sync"
if [[ "${1:-}" == "--check" ]]; then
    MODE="check"
elif [[ -n "${1:-}" ]]; then
    echo "사용법: $0 [--check]" >&2
    exit 1
fi

command -v cargo >/dev/null 2>&1 || { echo "cargo 가 PATH 에 없습니다." >&2; exit 1; }

if [[ "$MODE" == "check" ]]; then
    if [[ ! -d vendor ]]; then
        echo "vendor/ 디렉토리가 없습니다. 먼저 $0 을 실행하세요." >&2
        exit 1
    fi
    echo "==> vendor/ 무결성 검증 (offline, --locked)"
    cargo metadata --locked --offline --format-version 1 >/dev/null
    cargo fetch --locked --offline
    echo "==> OK: vendor/ 가 Cargo.lock 과 일치합니다."
    exit 0
fi

echo "==> Cargo.lock 갱신 + vendor/ 재동기화 (crates.io 접근 필요)"
# .cargo/config.toml 의 net.offline / source replacement 를 CLI 로 일시 해제.
cargo vendor --locked \
    --config 'net.offline=false' \
    --config 'source.crates-io.replace-with=""' \
    vendor >/dev/null 2>&1 || \
cargo vendor \
    --config 'net.offline=false' \
    --config 'source.crates-io.replace-with=""' \
    vendor >/dev/null

VENDOR_SIZE=$(du -sh vendor 2>/dev/null | cut -f1)
VENDOR_COUNT=$(find vendor -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
echo ""
echo "==> 완료"
echo "    vendor/ 크기      : ${VENDOR_SIZE}"
echo "    벤더된 크레이트   : ${VENDOR_COUNT} 개"
echo ""
echo "다음 단계 (4-게이트 + 공급망):"
echo "    cargo build  --workspace --all-targets --locked"
echo "    cargo test   --workspace --locked"
echo "    cargo clippy --workspace --all-targets -- -D warnings"
echo "    cargo fmt    --all -- --check"
echo "    cargo deny   check"
echo ""
echo "Cargo.toml / Cargo.lock / vendor/ 를 함께 커밋하세요."
