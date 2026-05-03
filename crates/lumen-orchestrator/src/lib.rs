//! 다중 에이전트 슈퍼바이저.
//!
//! v0.2 부터 완전 격리된 다중 에이전트 운영을 지원합니다: 각 에이전트는
//! 자체 tokio 태스크에서 실행되고, peer 상태에 대한 참조를 보유하지 않으며,
//! [`Orchestrator`] 의 별-라우팅 인터-에이전트 메시징으로만 peer 에 도달
//! 합니다. 등록부의 에이전트 ID 는 고유하며, 알려진 id 로 더블-spawn 하면
//! 거부됩니다. 에이전트별 inbox 는 bounded `mpsc` 채널이라 잘못된 sender 가
//! 메모리를 고갈시키는 대신 backpressure 를 적용합니다.
//!
//! v0.4 부터 capability 게이팅된 인터-에이전트 메시징을 지원합니다.
//! [`MessagePolicy`] 가 부착된 [`Orchestrator`] 는 모든 송신에 대해
//! `Resource::AgentMessage(recipient)` capability 를 요구합니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod supervisor;

pub use supervisor::{
    AgentHandle, AgentInbox, AgentSender, AgentSpec, InterAgentMessage, MessagePolicy,
    Orchestrator, DEFAULT_CMD_DEPTH, DEFAULT_INBOX_DEPTH,
};
