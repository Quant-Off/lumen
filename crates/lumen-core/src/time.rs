//! Capability 만료와 audit 이벤트가 사용하는 거친 밀리초 단위 timestamp.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Unix epoch 이후의 밀리초.
///
/// 의도적으로 `chrono` / `time` 의존성을 피해 의존성을 작게 유지합니다.
/// Capability 만료에는 단조에 가까운 거친 시간만 필요합니다. epoch 이전
/// 시각은 0 으로, 매우 먼 미래는 `u64::MAX` 로 saturate 합니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Unix epoch - `0`.
    pub const ZERO: Self = Self(0);

    /// 표현 가능한 가장 먼 미래.
    pub const FOREVER: Self = Self(u64::MAX);

    /// 현재 wall-clock 시각을 캡처합니다.
    pub fn now() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        Self(ms)
    }

    /// Unix epoch 이후의 원시 밀리초로부터 생성.
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    /// Unix epoch 이후의 밀리초.
    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// 밀리초 단위 duration 을 더합니다 (overflow 시 saturate).
    pub const fn saturating_add_ms(self, delta_ms: u64) -> Self {
        Self(self.0.saturating_add(delta_ms))
    }
}
