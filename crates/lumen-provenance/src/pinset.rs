//! 모델 핀 자동 회전 (rotation) - grace period.
//!
//! 새 모델 버전 배포 시 즉시 이전 버전을 거부하면 in-flight 요청이 실패하고
//! 롤백 윈도우가 사라집니다. [`PinSet`] 은 두 개의 핀 (`current` 와 옵션
//! `previous`) 을 보관하며, 이전 핀에는 `expires_at` 이 할당되어 grace 가
//! 끝나기 전까지 양쪽 모두를 받아들입니다.
//!
//! 호출자는 [`PinSet::rotate`] 로 새 해시를 들어 올리고, [`PinSet::verify`]
//! 로 디스크 상의 모델 파일이 둘 중 하나와 일치하는지 확인합니다. grace
//! 윈도우 동안의 매칭은 [`PinAcceptance::Grace`] 로 분명히 표시되어 audit
//! 로그가 silent fallback 을 잡아낼 수 있습니다.
//!
//! 본 모듈은 stateless: serialize/deserialize 가능한 [`PinSet`] 인스턴스를
//! 호출자가 정책 파일과 함께 운반합니다 - Lumen 의 zero-trust 원칙에 따라
//! 디스크 상태에 대한 묵시적 trust 는 두지 않습니다.

use std::path::Path;

use lumen_core::{Blake3Hash, Error, Result, Timestamp};
use serde::{Deserialize, Serialize};

use crate::manifest::ModelManifest;

/// 단일 핀 (해시 + 메타데이터).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinEntry {
    /// 모델 파일의 BLAKE3 해시.
    pub hash: Blake3Hash,
    /// 자유 형식 버전 라벨 (semver 권장; audit 로그에 등장).
    #[serde(default)]
    pub version: Option<String>,
}

/// 회전 가능한 핀 집합.
///
/// `current` 는 항상 받아들여집니다. `previous` 는 `expires_at` 까지만
/// 받아들여집니다 - 그 이후의 매칭은 [`PinAcceptance::Rejected`] 로 거부됩니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinSet {
    /// 현재 활성 핀.
    pub current: PinEntry,
    /// 회전 이전의 핀과 그 grace 만료 시각.
    #[serde(default)]
    pub previous: Option<RetiredPin>,
}

/// 회전 후 grace 윈도우 동안만 받아들여지는 이전 핀.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetiredPin {
    /// 이전 핀.
    pub entry: PinEntry,
    /// grace 가 끝나는 시각. 이 시각 *이후* 의 매칭은 거부됩니다.
    pub expires_at: Timestamp,
}

/// `verify` 의 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinAcceptance {
    /// 현재 핀과 일치.
    Current,
    /// 이전 핀과 일치하며 grace 윈도우 내. 호출자는 audit 로그를 남기고
    /// 가능한 한 빠르게 새 모델로 회전해야 합니다.
    Grace {
        /// grace 가 끝나는 시각.
        expires_at: Timestamp,
    },
    /// 어느 핀과도 일치하지 않거나 이전 핀이 만료됨.
    Rejected {
        /// 사람이 읽을 수 있는 거부 사유.
        reason: String,
    },
}

impl PinSet {
    /// 단일 핀으로 시작 (회전 이력 없음).
    #[must_use]
    pub fn single(current: PinEntry) -> Self {
        Self {
            current,
            previous: None,
        }
    }

    /// 새 해시를 들어 올리고 기존 `current` 를 grace 윈도우와 함께 `previous`
    /// 로 강등합니다.
    ///
    /// `grace_period_ms` 가 0 이면 이전 핀은 즉시 만료되어 사실상 hard cut.
    /// `now` 는 호출자 결정론을 위해 명시적으로 받습니다 (테스트 친화).
    pub fn rotate(&mut self, new: PinEntry, now: Timestamp, grace_period_ms: u64) {
        let retired = RetiredPin {
            entry: std::mem::replace(&mut self.current, new),
            expires_at: now.saturating_add_ms(grace_period_ms),
        };
        self.previous = Some(retired);
    }

    /// 해시를 핀 집합과 비교합니다.
    pub fn verify(&self, hash: &Blake3Hash, now: Timestamp) -> PinAcceptance {
        if &self.current.hash == hash {
            return PinAcceptance::Current;
        }
        if let Some(retired) = &self.previous {
            if &retired.entry.hash == hash {
                if now < retired.expires_at {
                    return PinAcceptance::Grace {
                        expires_at: retired.expires_at,
                    };
                }
                return PinAcceptance::Rejected {
                    reason: format!(
                        "previous pin matched but grace expired at {} (now={})",
                        retired.expires_at.as_millis(),
                        now.as_millis()
                    ),
                };
            }
        }
        PinAcceptance::Rejected {
            reason: "hash matches no pin in set".to_string(),
        }
    }
}

/// 모델 파일을 핀 집합으로 검증합니다 - [`crate::verify_model`] 의 회전
/// 가능 변종.
///
/// `manifest` 는 형식 sniff 와 옵션 서명 검증에만 사용되며, 해시 매칭은
/// 핀 집합으로 수행됩니다. 매니페스트의 `hash` 필드는 무시됩니다 - 이전
/// 매니페스트를 그대로 두고도 핀 집합만 회전하면 무중단 배포가 가능합니다.
///
/// 통과 사유 ([`PinAcceptance::Current`] 또는 [`PinAcceptance::Grace`])
/// 가 호출자에게 반환되어 grace 매칭은 audit / 알림으로 escalate 할 수
/// 있습니다.
pub fn verify_model_against_pinset(
    path: &Path,
    manifest: &ModelManifest,
    pinset: &PinSet,
    now: Timestamp,
    trusted_signers: &[lumen_core::VerifyingKey],
) -> Result<(crate::ModelInfo, PinAcceptance)> {
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();

    let actual = Blake3Hash::of_file(path)?;
    let acceptance = pinset.verify(&actual, now);
    let accepted = match &acceptance {
        PinAcceptance::Current | PinAcceptance::Grace { .. } => true,
        PinAcceptance::Rejected { reason } => {
            return Err(Error::Provenance(format!(
                "pinset rejected hash {actual} for {}: {reason}",
                manifest.name
            )));
        }
    };
    debug_assert!(accepted);

    // trusted_signers 가 비어있지 않다면 매니페스트는 반드시 서명되어
    // 있어야 합니다. 이전 구현은 signature/signer 가 None 인 매니페스트에
    // 대해 검증을 silently skip 하여, pinset 의 grace 윈도가 느슨한 경우
    // 미서명 매니페스트가 통과될 수 있는 결함이 있었습니다.
    match (&manifest.signature, &manifest.signer) {
        (Some(sig), Some(signer)) => {
            let body = manifest.signing_payload()?;
            let mut ok = false;
            for trusted in trusted_signers {
                if trusted == signer && trusted.verify(&body, sig).is_ok() {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return Err(Error::Provenance(
                    "signature did not verify against any trusted signer".into(),
                ));
            }
        }
        _ if !trusted_signers.is_empty() => {
            return Err(Error::Provenance(
                "manifest is unsigned but trusted signers were configured".into(),
            ));
        }
        _ => {
            // trusted_signers 가 비어 있고 매니페스트도 미서명 - pinset
            // 검증만으로 진행 (호출자가 명시적으로 unsigned 모드 선택).
        }
    }

    match manifest.format {
        crate::Format::Safetensors => {
            let _ = crate::safetensors_check::sniff(path)?;
        }
        crate::Format::Onnx => {
            let _ = crate::onnx::sniff(path)?;
        }
        crate::Format::Gguf => {
            let _ = crate::gguf::sniff(path)?;
        }
    }

    Ok((
        crate::ModelInfo {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            format: manifest.format,
            hash: actual,
            size_bytes,
        },
        acceptance,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> Blake3Hash {
        Blake3Hash([b; 32])
    }

    #[test]
    fn current_matches_immediately() {
        let p = PinSet::single(PinEntry {
            hash: h(0xAA),
            version: Some("v1".into()),
        });
        assert_eq!(
            p.verify(&h(0xAA), Timestamp::from_millis(0)),
            PinAcceptance::Current
        );
    }

    #[test]
    fn unknown_hash_rejected() {
        let p = PinSet::single(PinEntry {
            hash: h(0xAA),
            version: None,
        });
        match p.verify(&h(0xBB), Timestamp::from_millis(0)) {
            PinAcceptance::Rejected { .. } => {}
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn previous_accepted_during_grace() {
        let mut p = PinSet::single(PinEntry {
            hash: h(0xAA),
            version: Some("v1".into()),
        });
        p.rotate(
            PinEntry {
                hash: h(0xBB),
                version: Some("v2".into()),
            },
            Timestamp::from_millis(1_000),
            60_000, // 60s grace
        );
        // 새 모델은 항상 OK.
        assert_eq!(
            p.verify(&h(0xBB), Timestamp::from_millis(1_500)),
            PinAcceptance::Current
        );
        // 이전 모델은 grace 안에서 OK.
        match p.verify(&h(0xAA), Timestamp::from_millis(1_500)) {
            PinAcceptance::Grace { expires_at } => {
                assert_eq!(expires_at, Timestamp::from_millis(61_000));
            }
            other => panic!("expected Grace, got {other:?}"),
        }
    }

    #[test]
    fn previous_rejected_after_grace() {
        let mut p = PinSet::single(PinEntry {
            hash: h(0xAA),
            version: Some("v1".into()),
        });
        p.rotate(
            PinEntry {
                hash: h(0xBB),
                version: Some("v2".into()),
            },
            Timestamp::from_millis(1_000),
            60_000,
        );
        // grace 끝난 후 (정확히 expires_at 이후).
        match p.verify(&h(0xAA), Timestamp::from_millis(61_001)) {
            PinAcceptance::Rejected { .. } => {}
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn rotate_chains() {
        let mut p = PinSet::single(PinEntry {
            hash: h(0x01),
            version: Some("v1".into()),
        });
        // v1 → v2
        p.rotate(
            PinEntry {
                hash: h(0x02),
                version: Some("v2".into()),
            },
            Timestamp::from_millis(0),
            10_000,
        );
        // v2 → v3 - v1 은 chained-out.
        p.rotate(
            PinEntry {
                hash: h(0x03),
                version: Some("v3".into()),
            },
            Timestamp::from_millis(20_000),
            10_000,
        );
        // v1 은 더 이상 알려지지 않음.
        match p.verify(&h(0x01), Timestamp::from_millis(20_000)) {
            PinAcceptance::Rejected { .. } => {}
            other => panic!("v1 should be unknown, got {other:?}"),
        }
        // v2 는 새로운 grace 안.
        match p.verify(&h(0x02), Timestamp::from_millis(20_000)) {
            PinAcceptance::Grace { .. } => {}
            other => panic!("expected Grace, got {other:?}"),
        }
        // v3 는 current.
        assert_eq!(
            p.verify(&h(0x03), Timestamp::from_millis(20_000)),
            PinAcceptance::Current
        );
    }

    #[test]
    fn zero_grace_is_hard_cut() {
        let mut p = PinSet::single(PinEntry {
            hash: h(0xAA),
            version: None,
        });
        p.rotate(
            PinEntry {
                hash: h(0xBB),
                version: None,
            },
            Timestamp::from_millis(100),
            0,
        );
        // grace 가 0 이라면 같은 시각에 이전 핀은 이미 만료.
        match p.verify(&h(0xAA), Timestamp::from_millis(100)) {
            PinAcceptance::Rejected { .. } => {}
            other => panic!("expected Rejected with zero grace, got {other:?}"),
        }
    }
}
