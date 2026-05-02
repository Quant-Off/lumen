//! AMD SEV-SNP Attestation Report v2 와이어 포맷 파서.
//!
//! 참고: AMD *SEV Secure Nested Paging Firmware ABI Specification*. 보고서는
//! 정확히 1184 바이트 고정 길이입니다. 본 파서는 헤더 / 측정값 / 서명 영역의
//! 길이 일관성만 확인하며, ECDSA-P384 서명 검증은 v0.4 작업입니다.
//!
//! 핵심 필드 (LE 정수):
//!
//! | offset | size | field           |
//! |-------:|-----:|-----------------|
//! |    0   |   4  | version (= 2)   |
//! |    4   |   4  | guest_svn       |
//! |    8   |   8  | policy          |
//! |   16   |  16  | family_id       |
//! |   32   |  16  | image_id        |
//! |   48   |   4  | vmpl            |
//! |   80   |  64  | report_data     |
//! |  144   |  48  | measurement     |
//! |  192   |  32  | host_data       |
//! |  224   |  48  | id_key_digest   |
//! |  672   |  72  | signature_r     |
//! |  744   |  72  | signature_s     |
//! | total  | 1184 |                 |

use serde::{Deserialize, Serialize};

use crate::AttestationError;

/// SEV-SNP 보고서 v2 의 정확한 길이.
pub const SEV_SNP_REPORT_SIZE: usize = 1184;

/// 측정값 길이.
pub const MEASUREMENT_SIZE: usize = 48;
/// REPORT_DATA 길이.
pub const REPORT_DATA_SIZE: usize = 64;
/// HOST_DATA 길이.
pub const HOST_DATA_SIZE: usize = 32;

/// SEV-SNP 보고서의 핵심 필드.
///
/// 32 바이트를 넘는 byte 배열은 `Vec<u8>` 로 노출합니다 - serde derive 의
/// 안정 지원 범위 밖이라서. 길이는 [`Self::parse`] 가 보장합니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SevSnpReport {
    /// Guest SVN (security version).
    pub guest_svn: u32,
    /// Guest policy (확장된 정책 비트맵).
    pub policy: u64,
    /// 게스트 가족 식별자.
    pub family_id: [u8; 16],
    /// 게스트 이미지 식별자.
    pub image_id: [u8; 16],
    /// VMPL (virtual machine privilege level) 0..3.
    pub vmpl: u32,
    /// 호스트와 게스트가 합의한 64 바이트 데이터.
    pub report_data: Vec<u8>,
    /// 게스트 launch measurement (48 바이트).
    pub measurement: Vec<u8>,
    /// 호스트가 launch 시점에 기록한 32 바이트 데이터.
    pub host_data: [u8; HOST_DATA_SIZE],
    /// `id_key` 의 SHA-384 다이제스트 (48 바이트).
    pub id_key_digest: Vec<u8>,
    /// ECDSA P-384 r 값 (72 바이트, 검증은 v0.4).
    pub signature_r: Vec<u8>,
    /// ECDSA P-384 s 값 (72 바이트, 검증은 v0.4).
    pub signature_s: Vec<u8>,
}

impl SevSnpReport {
    /// 1184 바이트 입력을 파싱합니다.
    pub fn parse(bytes: &[u8]) -> Result<Self, AttestationError> {
        if bytes.len() != SEV_SNP_REPORT_SIZE {
            return Err(AttestationError::TooShort {
                field: "sev-snp report",
                got: bytes.len(),
                need: SEV_SNP_REPORT_SIZE,
            });
        }
        let version = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if version != 2 {
            return Err(AttestationError::BadMagic("sev-snp version != 2"));
        }
        let guest_svn = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let policy = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let mut family_id = [0u8; 16];
        family_id.copy_from_slice(&bytes[16..32]);
        let mut image_id = [0u8; 16];
        image_id.copy_from_slice(&bytes[32..48]);
        let vmpl = u32::from_le_bytes([bytes[48], bytes[49], bytes[50], bytes[51]]);
        if vmpl > 3 {
            return Err(AttestationError::BadMagic("sev-snp vmpl > 3"));
        }
        let report_data = bytes[80..80 + REPORT_DATA_SIZE].to_vec();
        let measurement = bytes[144..144 + MEASUREMENT_SIZE].to_vec();
        let mut host_data = [0u8; HOST_DATA_SIZE];
        host_data.copy_from_slice(&bytes[192..192 + HOST_DATA_SIZE]);
        let id_key_digest = bytes[224..224 + 48].to_vec();
        let signature_r = bytes[672..672 + 72].to_vec();
        let signature_s = bytes[744..744 + 72].to_vec();
        Ok(Self {
            guest_svn,
            policy,
            family_id,
            image_id,
            vmpl,
            report_data,
            measurement,
            host_data,
            id_key_digest,
            signature_r,
            signature_s,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 잘 형성된 보고서를 만드는 헬퍼.
    pub(super) fn fixture(measurement_byte: u8, vmpl: u32) -> Vec<u8> {
        let mut buf = vec![0u8; SEV_SNP_REPORT_SIZE];
        buf[0..4].copy_from_slice(&2u32.to_le_bytes());
        buf[4..8].copy_from_slice(&3u32.to_le_bytes()); // guest_svn
        buf[8..16].copy_from_slice(&0xDEAD_BEEFu64.to_le_bytes()); // policy
        buf[48..52].copy_from_slice(&vmpl.to_le_bytes());
        buf[80..80 + REPORT_DATA_SIZE].fill(0xAB);
        buf[144..144 + MEASUREMENT_SIZE].fill(measurement_byte);
        buf[192..192 + HOST_DATA_SIZE].fill(0x55);
        buf
    }

    #[test]
    fn parses_well_formed() {
        let buf = fixture(0x42, 1);
        let r = SevSnpReport::parse(&buf).unwrap();
        assert_eq!(r.guest_svn, 3);
        assert_eq!(r.policy, 0xDEAD_BEEF);
        assert_eq!(r.vmpl, 1);
        assert_eq!(r.measurement[0], 0x42);
        assert_eq!(r.measurement[MEASUREMENT_SIZE - 1], 0x42);
    }

    #[test]
    fn rejects_wrong_length() {
        let buf = vec![0u8; SEV_SNP_REPORT_SIZE - 1];
        let err = SevSnpReport::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::TooShort { .. }));
    }

    #[test]
    fn rejects_bad_version() {
        let mut buf = fixture(0, 0);
        buf[0] = 0xFF;
        let err = SevSnpReport::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::BadMagic(_)));
    }

    #[test]
    fn rejects_bad_vmpl() {
        let buf = fixture(0, 5);
        let err = SevSnpReport::parse(&buf).unwrap_err();
        assert!(matches!(err, AttestationError::BadMagic(_)));
    }
}
