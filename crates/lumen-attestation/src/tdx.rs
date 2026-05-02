//! Intel TDX Quote v4 와이어 포맷 파서.
//!
//! 참고: Intel *Trust Domain Extensions Module* spec (DCAP). 본 파서는
//! Quote v4 의 헤더와 본문 (`TD_REPORT`) 구조 일관성만 검증합니다 - PCK 인증서
//! 체인의 X.509 검증 및 PCS 의 quote signature 검증은 v0.4 작업입니다.
//!
//! 헤더 (632 바이트) 레이아웃 (LE 정수, 4-바이트 정렬):
//!
//! | offset | size | field         | meaning                        |
//! |-------:|-----:|---------------|--------------------------------|
//! |    0   |   4  | version       | 0x00000004                     |
//! |    4   |   4  | att_key_type  | 2 = ECDSA-P256                 |
//! |    8   |   4  | tee_type      | 0x81 = TDX                     |
//! |   12   |   2  | qe_svn        | QE security version            |
//! |   14   |   2  | pce_svn       | PCE security version           |
//! |   16   |  16  | qe_vendor_id  | UUID                           |
//! |   32   |  20  | user_data     | platform-defined               |
//! |   52   | 580  | TD_REPORT     | TD measurement bundle          |
//!
//! TD_REPORT 안의 핵심 필드:
//! - `mrtd` (offset 16..64 of TD_REPORT): TD measurement (48 바이트)
//! - `report_data` (offset 568..632 of TD_REPORT): 64 바이트 사용자 데이터
//!
//! 헤더 뒤에는 4 바이트 `signed_data_size` 와 그 길이만큼의 인증 데이터가
//! 붙지만, v0.3 에서는 길이 검증만 수행합니다.

use serde::{Deserialize, Serialize};

use crate::AttestationError;

/// TDX Quote v4 헤더 크기.
pub const TDX_QUOTE_HEADER_SIZE: usize = 632;

/// TDX Quote v4 매직 (version 필드).
pub const TDX_QUOTE_MAGIC: u32 = 4;

/// TDX TEE type 식별자.
pub const TDX_TEE_TYPE: u32 = 0x0000_0081;

/// MRTD (TD measurement) 길이.
pub const MRTD_SIZE: usize = 48;

/// REPORTDATA 길이.
pub const REPORT_DATA_SIZE: usize = 64;

/// Quote 헤더 + TD_REPORT 의 핵심 필드만 노출.
///
/// 큰 byte 배열은 serde derive 의 안정적 지원 범위 (≤ 32 바이트) 밖이므로
/// `Vec<u8>` 로 보유하고 길이는 파서가 보장합니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TdxQuote {
    /// QE security version.
    pub qe_svn: u16,
    /// PCE security version.
    pub pce_svn: u16,
    /// QE vendor UUID (16 바이트).
    pub qe_vendor_id: [u8; 16],
    /// 플랫폼별 user data (20 바이트).
    pub user_data: Vec<u8>,
    /// MRTD - TD measurement (48 바이트).
    pub mrtd: Vec<u8>,
    /// 사용자/호스트가 합의한 64-바이트 데이터.
    pub report_data: Vec<u8>,
    /// 서명/인증서 데이터 (v0.4 에서 검증). 원본 바이트 그대로 보존.
    pub signed_data: Vec<u8>,
}

impl TdxQuote {
    /// 헤더 + 인증 데이터를 파싱합니다.
    pub fn parse(bytes: &[u8]) -> Result<Self, AttestationError> {
        if bytes.len() < TDX_QUOTE_HEADER_SIZE {
            return Err(AttestationError::TooShort {
                field: "tdx header",
                got: bytes.len(),
                need: TDX_QUOTE_HEADER_SIZE,
            });
        }
        let version = read_u32_le(bytes, 0);
        if version != TDX_QUOTE_MAGIC {
            return Err(AttestationError::BadMagic("tdx version != 4"));
        }
        let tee_type = read_u32_le(bytes, 8);
        if tee_type != TDX_TEE_TYPE {
            return Err(AttestationError::BadMagic("tdx tee_type != 0x81"));
        }
        let qe_svn = u16::from_le_bytes([bytes[12], bytes[13]]);
        let pce_svn = u16::from_le_bytes([bytes[14], bytes[15]]);
        let mut qe_vendor_id = [0u8; 16];
        qe_vendor_id.copy_from_slice(&bytes[16..32]);
        let user_data = bytes[32..52].to_vec();

        // TD_REPORT 는 offset 52 부터 580 바이트.
        let td_report = &bytes[52..52 + 580];
        // td_report 안에서 mrtd 는 offset 16..64.
        let mrtd = td_report[16..16 + MRTD_SIZE].to_vec();
        // report_data 는 td_report 의 마지막 64 바이트.
        let report_data = td_report[580 - REPORT_DATA_SIZE..].to_vec();

        // 헤더 뒤 4 바이트는 signed_data_size.
        if bytes.len() < TDX_QUOTE_HEADER_SIZE + 4 {
            return Err(AttestationError::TooShort {
                field: "tdx signed_data_size",
                got: bytes.len(),
                need: TDX_QUOTE_HEADER_SIZE + 4,
            });
        }
        let signed_data_size = read_u32_le(bytes, TDX_QUOTE_HEADER_SIZE) as usize;
        let signed_start = TDX_QUOTE_HEADER_SIZE + 4;
        let signed_end = signed_start
            .checked_add(signed_data_size)
            .ok_or(AttestationError::LengthMismatch("tdx signed overflow"))?;
        if signed_end != bytes.len() {
            return Err(AttestationError::LengthMismatch(
                "tdx signed length 과 buffer 끝 불일치",
            ));
        }
        let signed_data = bytes[signed_start..signed_end].to_vec();

        Ok(Self {
            qe_svn,
            pce_svn,
            qe_vendor_id,
            user_data,
            mrtd,
            report_data,
            signed_data,
        })
    }
}

fn read_u32_le(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 잘 형성된 더미 Quote 를 생성하는 헬퍼 - 테스트 전용.
    pub(super) fn fixture(signed_data_len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; TDX_QUOTE_HEADER_SIZE + 4 + signed_data_len];
        buf[0..4].copy_from_slice(&TDX_QUOTE_MAGIC.to_le_bytes());
        // att_key_type = 2 (ECDSA-P256)
        buf[4..8].copy_from_slice(&2u32.to_le_bytes());
        buf[8..12].copy_from_slice(&TDX_TEE_TYPE.to_le_bytes());
        // qe_svn / pce_svn
        buf[12..14].copy_from_slice(&7u16.to_le_bytes());
        buf[14..16].copy_from_slice(&13u16.to_le_bytes());
        // qe_vendor_id
        for (i, b) in buf[16..32].iter_mut().enumerate() {
            *b = i as u8;
        }
        // user_data
        for (i, b) in buf[32..52].iter_mut().enumerate() {
            *b = (0xA0 + i) as u8;
        }
        // TD_REPORT.mrtd at offset 52 + 16 = 68
        for (i, b) in buf[68..68 + MRTD_SIZE].iter_mut().enumerate() {
            *b = (0xC0 + i) as u8;
        }
        // TD_REPORT.report_data at offset 52 + 580 - 64 = 568
        for (i, b) in buf[568..568 + REPORT_DATA_SIZE].iter_mut().enumerate() {
            *b = (0xE0 + i) as u8;
        }
        // signed_data_size
        buf[TDX_QUOTE_HEADER_SIZE..TDX_QUOTE_HEADER_SIZE + 4]
            .copy_from_slice(&(signed_data_len as u32).to_le_bytes());
        buf
    }

    #[test]
    fn parses_well_formed_fixture() {
        let buf = fixture(32);
        let q = TdxQuote::parse(&buf).unwrap();
        assert_eq!(q.qe_svn, 7);
        assert_eq!(q.pce_svn, 13);
        assert_eq!(q.signed_data.len(), 32);
        assert_eq!(q.mrtd[0], 0xC0);
        assert_eq!(q.report_data[0], 0xE0);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = fixture(0);
        buf[0] = 0xFF;
        let err = TdxQuote::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::BadMagic(_)));
    }

    #[test]
    fn rejects_bad_tee_type() {
        let mut buf = fixture(0);
        buf[8] = 0xAA;
        let err = TdxQuote::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::BadMagic(_)));
    }

    #[test]
    fn rejects_signed_length_mismatch() {
        let mut buf = fixture(32);
        // signed_data_size 를 100 으로 변경하지만 실제는 32 바이트.
        buf[TDX_QUOTE_HEADER_SIZE..TDX_QUOTE_HEADER_SIZE + 4]
            .copy_from_slice(&100u32.to_le_bytes());
        let err = TdxQuote::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::LengthMismatch(_)));
    }

    #[test]
    fn rejects_truncated_header() {
        let buf = vec![0u8; 100];
        let err = TdxQuote::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::TooShort { .. }));
    }
}
