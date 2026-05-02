//! 공통 에러 타입. 각 서브시스템은 자신의 네이티브 에러를 여기 정의된 variant
//! 로 매핑합니다.

use thiserror::Error;

/// Lumen 의 통합 에러 타입. 서브시스템은 새 variant 를 만들기보다
/// 기존 variant 로 매핑하는 것을 선호합니다.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// I/O 실패 (파일 시스템, 네트워크).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// 16진수 디코딩 실패.
    #[error("hex decode: {0}")]
    Hex(#[from] hex::FromHexError),

    /// 암호학적 연산 실패 (서명 검증, 키 파싱 등).
    #[error("crypto: {0}")]
    Crypto(String),

    /// 직렬화 또는 역직렬화 실패.
    #[error("decode: {0}")]
    Decode(String),

    /// Capability 토큰이 손상, 만료, 리플레이 또는 잘못된 audience 였음.
    #[error("capability: {0}")]
    Capability(String),

    /// 정책이 액션을 거부했음.
    #[error("policy: {0}")]
    Policy(String),

    /// Provenance 또는 SBOM 검증 실패.
    #[error("provenance: {0}")]
    Provenance(String),

    /// 방어 엔진이 입력을 차단했음.
    #[error("defense: {0}")]
    Defense(String),

    /// zkML 증명 또는 검증 실패.
    #[error("zkml: {0}")]
    Zkml(String),

    /// 추론 엔진 실패.
    #[error("inference: {0}")]
    Inference(String),

    /// WASM 샌드박스 실패.
    #[error("sandbox: {0}")]
    Sandbox(String),

    /// 채널 송수신 실패.
    #[error("channel: {0}")]
    Channel(String),

    /// 에이전트 런타임 실패.
    #[error("agent: {0}")]
    Agent(String),

    /// 임의의 서브시스템에서 표면화된 설정/입력 검증 오류.
    #[error("invalid: {0}")]
    Invalid(String),

    /// 이번 빌드에서는 아직 구현되지 않은 기능.
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
}

/// 워크스페이스 전체에서 사용되는 편의 별칭.
pub type Result<T> = std::result::Result<T, Error>;
