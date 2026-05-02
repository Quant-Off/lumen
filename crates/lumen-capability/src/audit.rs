//! Capability 결정에 대한 구조화된 tracing 헬퍼.

use crate::{Action, Capability};

/// "allow" audit 레코드 발행.
pub fn allow(cap: &Capability, action: &Action<'_>) {
    tracing::info!(
        target: "lumen.capability",
        decision = "allow",
        cap_id = %cap.body.id,
        agent = %cap.body.audience,
        issuer = %cap.body.issuer,
        action = ?action,
    );
}

/// 사유 와 함께 "deny" audit 레코드 발행.
pub fn deny(cap: Option<&Capability>, action: &Action<'_>, reason: &str) {
    if let Some(cap) = cap {
        tracing::warn!(
            target: "lumen.capability",
            decision = "deny",
            cap_id = %cap.body.id,
            agent = %cap.body.audience,
            issuer = %cap.body.issuer,
            action = ?action,
            reason,
        );
    } else {
        tracing::warn!(
            target: "lumen.capability",
            decision = "deny",
            action = ?action,
            reason,
        );
    }
}
