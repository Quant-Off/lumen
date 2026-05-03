//! Mina (o1js) 검증기 source 생성.

use crate::VerifierMeta;

/// `RoutingVerifier.ts` 의 결정론적 source 를 반환합니다.
///
/// o1js (구 SnarkyJS) zkApp 로서, EVM 측과 동일한 두 게이트를 enforce 합니다.
/// Mina 는 자연 ZK 환경이므로 입력은 `Field` (snark-friendly prime field)
/// 이며 wrap-around 우려가 없어 EVM 의 `SCORE_BOUND` 에 해당하는 가드는
/// 필요 없습니다.
pub fn o1js_source(meta: &VerifierMeta) -> String {
    let circuit_id = &meta.circuit_id;
    format!(
        r#"// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Lumen RoutingVerifier (Mina, o1js)
// circuit_id = {circuit_id}
//
// halo2 backend 와 동일한 두 게이트를 강제합니다:
//   1. selected = bit * (score_b - score_a) + score_a
//   2. bit * (1 - bit) === 0
//
// v0.4 stub - succinct proof 검증으로 업그레이드 예정.

import {{
    Field,
    SmartContract,
    state,
    State,
    method,
    Bool,
    Provable,
}} from 'o1js';

export class RoutingVerifier extends SmartContract {{
    @state(Field) lastSelected = State<Field>();

    /// @notice halo2 backend 와 일치하는 회로 식별자.
    static CIRCUIT_ID: string = "{circuit_id}";

    init() {{
        super.init();
        this.lastSelected.set(Field(0));
    }}

    @method async verify(
        scoreA: Field,
        scoreB: Field,
        choiceBit: Field,
        selectedValue: Field,
    ) {{
        // 게이트 2: bit * (1 - bit) = 0  ⇒ bit ∈ {{ 0, 1 }}
        choiceBit.mul(Field(1).sub(choiceBit)).assertEquals(Field(0));

        // 게이트 1: selected == bit * (b - a) + a
        const expected = choiceBit.mul(scoreB.sub(scoreA)).add(scoreA);
        expected.assertEquals(selectedValue);

        this.lastSelected.set(selectedValue);
    }}
}}

export const RoutingVerifierMeta = {{
    circuitId: "{circuit_id}",
}};
"#
    )
}

/// Mina 측 deploy entry (o1js CLI 사용).
pub fn mina_deploy_script(meta: &VerifierMeta) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Lumen RoutingVerifier (Mina) deploy script - circuit_id={circuit_id}
#
# 요구 환경:
#   - zk (o1js CLI)         https://docs.minaprotocol.com/zkapps
#   - LUMEN_MINA_RPC        e.g. https://proxy.berkeley.minaexplorer.com/graphql
#   - LUMEN_MINA_FEE_PAYER  fee payer 의 키 alias (zk config 에 등록되어 있어야 함)
#
set -euo pipefail
: "${{LUMEN_MINA_RPC:?LUMEN_MINA_RPC 미설정}}"
: "${{LUMEN_MINA_FEE_PAYER:?LUMEN_MINA_FEE_PAYER 미설정}}"

# zk deploy 는 키 alias 와 GraphQL endpoint 를 사용해 zkApp account 를
# 활성화하고 contract 를 배포합니다.
zk deploy --network "$LUMEN_MINA_RPC" --fee-payer "$LUMEN_MINA_FEE_PAYER"
"#,
        circuit_id = meta.circuit_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o1js_contains_circuit_id() {
        let meta = VerifierMeta::for_circuit("lumen.routing.binary.v1");
        let src = o1js_source(&meta);
        assert!(src.contains("lumen.routing.binary.v1"));
        assert!(src.contains("class RoutingVerifier"));
        assert!(src.contains("assertEquals"));
    }

    #[test]
    fn mina_script_uses_circuit_id() {
        let m = VerifierMeta::for_circuit("c1");
        let s = mina_deploy_script(&m);
        assert!(s.contains("c1"));
        assert!(s.contains("zk deploy"));
    }
}
