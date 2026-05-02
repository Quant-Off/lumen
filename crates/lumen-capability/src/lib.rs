//! Lumen 의 Capability 기반 권한 시스템.
//!
//! Capability 는 위조 불가능한 서명된 토큰으로 다음을 묶습니다.
//!
//! - **audience** (어떤 에이전트가 이 capability 를 행사할 수 있는가),
//! - **resource** (파일, 네트워크 호스트, 도구, 추론 예산 등),
//! - **nonce** (리플레이 방어),
//! - **만료 시각**,
//! - 그리고 **issuer** 의 Ed25519 서명.
//!
//! [`PolicyEngine`] 은 권한이 필요한 액션이 실행되기 전에 이 토큰들을
//! 검증합니다. 통과한 capability 의 nonce 는 만료까지 기억되어 여전히
//! 유효한 토큰의 리플레이 시도를 막습니다.
//!
//! 모든 결정은 결정론적인 필드 이름으로 `tracing` audit 이벤트를 발행하며,
//! 외부 SIEM 도구가 별도 파서 없이 수신할 수 있습니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod audit;
pub mod capability;
pub mod policy;
pub mod resource;

pub use capability::{Capability, CapabilityBody};
pub use policy::{Action, PolicyEngine};
pub use resource::{HostPattern, PathPattern, Resource};
