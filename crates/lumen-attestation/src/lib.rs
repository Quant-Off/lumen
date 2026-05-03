//! TEE attestation 문서 와이어-포맷 파서.
//!
//! v0.3 의 목표는 *어떤 형태가 attestation 문서인지* 를 인지하고 길이/매직
//! 바이트/구조 일관성을 검사하는 것입니다. **암호학적 검증** - TDX quote
//! 의 PCK 인증서 체인 검증, SEV-SNP 의 VCEK 서명 검증 - 은 v0.4 마일스톤
//! 입니다. 그 시점까지는 [`AttestationDoc::verify`] 가 항상
//! [`Verdict::FormatOnly`] 를 반환합니다 - 진짜 검증된 것이 아닌 *형식만*
//! 통과한 상태임을 호출자가 명시적으로 인지하도록 강제합니다.
//!
//! 지원 형식:
//!
//! - **Intel TDX Quote v4** - `intel-tdx.org` 의 *Trust Domain Extensions*
//!   spec 에서 정의됨. 632 바이트 헤더 + 가변 길이 인증 데이터.
//! - **AMD SEV-SNP Attestation Report v2** - AMD 의 *SEV-SNP ABI Spec* 에서
//!   정의됨. 1184 바이트 고정 길이 보고서.
//!
//! 두 형식 모두 *측정값 (measurement)*, *플랫폼 ID*, *서명* 필드를 가지지만
//! 와이어 레이아웃이 매우 다릅니다. 본 모듈은 두 변종을 명시적으로 분리해
//! `AttestationDoc::Tdx(...)` / `AttestationDoc::SevSnp(...)` 로 표현합니다.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod sev_snp;
pub mod tdx;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use sev_snp::{SevSnpReport, SEV_SNP_REPORT_SIZE};
pub use tdx::{TdxQuote, TDX_QUOTE_HEADER_SIZE, TDX_QUOTE_MAGIC};

/// attestation 파싱/검증 실패.
#[derive(Debug, Error)]
pub enum AttestationError {
    /// 입력 길이가 형식의 최소 요구값보다 작습니다.
    #[error("입력이 너무 짧음: {field} (got {got}, need at least {need})")]
    TooShort {
        /// 어느 필드가 부족했는지.
        field: &'static str,
        /// 실제 길이.
        got: usize,
        /// 최소 요구 길이.
        need: usize,
    },
    /// 매직 바이트 / 버전 / 구조 식별자가 기대와 다릅니다.
    #[error("매직/버전 불일치: {0}")]
    BadMagic(&'static str),
    /// 알려지지 않은 플랫폼 식별자.
    #[error("알 수 없는 플랫폼")]
    UnknownPlatform,
    /// 길이 오버플로우 또는 본문이 헤더가 약속한 크기를 초과/미달.
    #[error("길이 모순: {0}")]
    LengthMismatch(&'static str),
}

/// 형식만 검증된 attestation 문서. 두 플랫폼을 분리합니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttestationDoc {
    /// Intel TDX Quote v4.
    Tdx(TdxQuote),
    /// AMD SEV-SNP Attestation Report v2.
    SevSnp(SevSnpReport),
}

impl AttestationDoc {
    /// 입력의 첫 바이트를 보고 두 형식 중 하나로 파싱을 시도합니다.
    pub fn parse(bytes: &[u8]) -> Result<Self, AttestationError> {
        // TDX 는 첫 4 바이트가 매직 0x00000004 (LE) 으로 시작합니다 (version=4 quote).
        if bytes.len() >= 4
            && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == TDX_QUOTE_MAGIC
        {
            return tdx::TdxQuote::parse(bytes).map(AttestationDoc::Tdx);
        }
        // SEV-SNP 보고서는 정확히 1184 바이트이고 version 필드 (offset 0..4) 가 2.
        if bytes.len() == SEV_SNP_REPORT_SIZE
            && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 2
        {
            return sev_snp::SevSnpReport::parse(bytes).map(AttestationDoc::SevSnp);
        }
        Err(AttestationError::UnknownPlatform)
    }

    /// 측정값 (measurement) 을 반환합니다 - TDX 의 MRTD, SEV-SNP 의 measurement.
    pub fn measurement(&self) -> &[u8] {
        match self {
            Self::Tdx(q) => &q.mrtd,
            Self::SevSnp(r) => &r.measurement,
        }
    }

    /// REPORT_DATA / report_data 필드 (호스트와 게스트가 합의한 64바이트 데이터).
    pub fn report_data(&self) -> &[u8] {
        match self {
            Self::Tdx(q) => &q.report_data,
            Self::SevSnp(r) => &r.report_data,
        }
    }
}

/// `AttestationDoc::verify` 의 결과.
///
/// `FormatOnly` 와 `CryptoVerified` 는 **명시적으로** 별개 variant 입니다 -
/// 형식 검증과 암호학적 검증이 호출자 코드에서 절대 혼동되지 않도록.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// 와이어 포맷이 잘 형성됨. 서명/인증서 체인은 *검증되지 않음*.
    /// v0.3 의 모든 호출은 이 값을 반환합니다.
    FormatOnly,
    /// 플랫폼 신뢰 앵커 기준 암호학적으로 검증됨. v0.4 에서 활성화.
    CryptoVerified,
    /// 형식이 잘못되었거나 검증 실패.
    Invalid,
}

impl AttestationDoc {
    /// 검증 (v0.3: 형식만).
    pub fn verify(&self) -> Verdict {
        // 파싱이 성공했다는 사실 자체가 형식이 통과했다는 뜻이므로 항상 FormatOnly.
        // v0.4 에서는 여기에 PCK chain 검증 / VCEK 서명 검증이 추가됩니다.
        Verdict::FormatOnly
    }
}
