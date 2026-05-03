//! EVM (Solidity) 검증기 source 생성.

use crate::VerifierMeta;

/// `RoutingVerifier.sol` 의 결정론적 source 를 반환합니다.
///
/// `verify(score_a, score_b, choice_bit, selected_value)` 가 다음 두 가지
/// 게이트를 강제합니다:
///
/// - `selected_value == bit * (score_b - score_a) + score_a`
/// - `bit * (1 - bit) == 0` (즉 `bit ∈ {0, 1}`)
///
/// 모든 산술은 `unchecked { ... }` 블록 안에서 수행되어 회로 도메인
/// (`pasta_curves::pallas::Base`) 의 modular arithmetic 을 closely 모방
/// 합니다 - 그러나 EVM 의 `uint256` mod 2^256 은 pallas Fp 와 다른 modulus
/// 이므로, 입력은 caller 가 작은 정수 (< 2^128) 로 제공해야 합니다.
/// `verify_with_bound` 가 이 boundary 를 강제하여 조용한 wrap-around 를
/// 거부합니다.
pub fn solidity_source(meta: &VerifierMeta) -> String {
    let circuit_id = &meta.circuit_id;
    format!(
        r#"// SPDX-License-Identifier: Apache-2.0 OR MIT
pragma solidity ^0.8.20;

/// @title Lumen RoutingVerifier
/// @notice {circuit_id} 회로의 제약 만족을 on-chain 에서 재검증.
/// @dev   v0.4 stub: KZG-backed succinct verifier 로 업그레이드 예정.
///        ABI 안정성을 위해 circuit_id 가 storage 에 박혀 있습니다.
contract RoutingVerifier {{
    /// @notice halo2 backend 와 일치하는 회로 식별자.
    string public constant CIRCUIT_ID = "{circuit_id}";

    /// @notice 검증 통과 시 발생하는 이벤트.
    event Verified(
        uint256 indexed scoreA,
        uint256 indexed scoreB,
        uint256 choiceBit,
        uint256 selectedValue
    );

    /// @notice score 입력의 안전한 상한.
    /// @dev    EVM uint256 산술과 pallas Fp 의 modulus 가 다르므로, 작은
    ///         정수 도메인 (< 2^128) 으로 제한해 wrap-around 를 차단.
    uint256 public constant SCORE_BOUND = 1 << 128;

    error BitOutOfRange();
    error SelectedMismatch();
    error InputOutOfBound();

    /// @notice `(score_a, score_b, choice_bit, selected_value)` 를 검증.
    /// @return true on success; revert on failure (audit-friendly).
    function verify(
        uint256 scoreA,
        uint256 scoreB,
        uint256 choiceBit,
        uint256 selectedValue
    ) external returns (bool) {{
        if (scoreA >= SCORE_BOUND || scoreB >= SCORE_BOUND || selectedValue >= SCORE_BOUND) {{
            revert InputOutOfBound();
        }}
        if (choiceBit > 1) {{
            revert BitOutOfRange();
        }}
        // selected = bit * (b - a) + a  (unchecked: 작은 정수 도메인 보장됨).
        unchecked {{
            uint256 expected;
            if (choiceBit == 0) {{
                expected = scoreA;
            }} else {{
                expected = scoreB;
            }}
            if (expected != selectedValue) {{
                revert SelectedMismatch();
            }}
        }}
        emit Verified(scoreA, scoreB, choiceBit, selectedValue);
        return true;
    }}

    /// @notice 변경 효과 없이 검증만 수행 (off-chain 호출용 view).
    function verifyView(
        uint256 scoreA,
        uint256 scoreB,
        uint256 choiceBit,
        uint256 selectedValue
    ) external pure returns (bool) {{
        if (scoreA >= SCORE_BOUND || scoreB >= SCORE_BOUND || selectedValue >= SCORE_BOUND) {{
            return false;
        }}
        if (choiceBit > 1) {{
            return false;
        }}
        if (choiceBit == 0) {{
            return scoreA == selectedValue;
        }}
        return scoreB == selectedValue;
    }}
}}
"#
    )
}

/// `forge create` 기반 deploy script. RPC 와 private-key 환경 변수만
/// 채우면 그대로 실행 가능.
pub fn forge_deploy_script(meta: &VerifierMeta) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Lumen RoutingVerifier (EVM) deploy script - circuit_id={circuit_id}
#
# 요구 환경:
#   - forge (Foundry)         https://getfoundry.sh
#   - LUMEN_RPC_URL           e.g. https://sepolia.example.org
#   - LUMEN_DEPLOY_PRIVKEY    배포자 EOA 의 hex private key
#
# 본 스크립트는 RoutingVerifier.sol 한 개 컨트랙트를 배포하고, 결과 주소를
# stdout 으로 출력합니다. CI/CD 파이프라인에서 호출하면 결정론적이며 외부
# state 의존이 없습니다.
set -euo pipefail
: "${{LUMEN_RPC_URL:?LUMEN_RPC_URL 미설정}}"
: "${{LUMEN_DEPLOY_PRIVKEY:?LUMEN_DEPLOY_PRIVKEY 미설정}}"

forge create \
    --rpc-url "$LUMEN_RPC_URL" \
    --private-key "$LUMEN_DEPLOY_PRIVKEY" \
    --broadcast \
    src/RoutingVerifier.sol:RoutingVerifier
"#,
        circuit_id = meta.circuit_id
    )
}

/// `foundry.toml` 결정론적 출력.
pub fn foundry_toml() -> String {
    r#"[profile.default]
src = "src"
out = "out"
libs = ["lib"]
optimizer = true
optimizer_runs = 200
solc_version = "0.8.20"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solidity_contains_circuit_id() {
        let meta = VerifierMeta::for_circuit("lumen.routing.binary.v1");
        let src = solidity_source(&meta);
        assert!(src.contains("lumen.routing.binary.v1"));
        assert!(src.contains("contract RoutingVerifier"));
        assert!(src.contains("function verify"));
        assert!(src.contains("BitOutOfRange"));
    }

    #[test]
    fn forge_script_uses_circuit_id_in_comment() {
        let meta = VerifierMeta::for_circuit("lumen.routing.binary.v1");
        let s = forge_deploy_script(&meta);
        assert!(s.contains("lumen.routing.binary.v1"));
        assert!(s.contains("forge create"));
    }

    #[test]
    fn solidity_is_deterministic() {
        let m = VerifierMeta::for_circuit("c1");
        let a = solidity_source(&m);
        let b = solidity_source(&m);
        assert_eq!(a, b, "solidity emission must be deterministic");
    }
}
