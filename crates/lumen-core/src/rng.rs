//! Lumen의 RNG 추상화 모듈입니다.
//!
//! 폐쇄형(Air-Gapped) iso-light-k0 마이크로커널 환경 호환을 위해 외부
//! `rand` / `rand_core` 크레이트 대신 `elib-k0-nt/rng` 의 OS 엔트로피
//! 수집과 NIST SP 800-90A Hash_DRBG 를 그대로 노출하는 얇은 래퍼입니다.
//!
//! # 트레이트와 구체 타입
//!
//! - [`Rng`] : 단일 메서드 `fill_bytes` 만 갖는 최소 트레이트. 임의의
//!   고정 길이 식별자 / 키 시드 / nonce 채움에 사용됩니다.
//! - [`OsRng`] : OS 엔트로피 (Linux `getrandom`, macOS / *BSD `arc4random`,
//!   Windows BCrypt 등) 를 직접 사용하는 무상태 RNG. 프로덕션 기본값.
//! - [`HashDrbg`] : OS 엔트로피로 한번 시드된 SHA-256 기반 Hash_DRBG.
//!   동일 페어가 다량의 키 자료를 생성할 때 외부 entropy 호출 횟수를
//!   줄이고 reseed 인터벌을 명시적으로 제어할 수 있습니다.

use elib_rng::{DrbgError, HashDRBGSHA256};
use elib_zeroize::Zeroize;

use crate::error::{Error, Result};

/// 단일 책임의 RNG 트레이트입니다.
///
/// `rand_core::RngCore` 를 대체합니다. 호출자는 이 트레이트만 의존하면
/// 테스트에서는 결정론적 RNG (예: 직접 시딩한 [`HashDrbg`]) 를, 프로덕션
/// 에서는 [`OsRng`] 를 주입할 수 있습니다.
pub trait Rng {
    /// 주어진 슬라이스를 암호학적으로 안전한 난수로 채웁니다.
    ///
    /// # Panics
    /// OS 엔트로피 소스가 사용 불가 등 회복 불가능한 실패 시 panic 합니다.
    /// 회복 가능한 처리가 필요한 경우 [`Rng::try_fill_bytes`] 를 사용하세요.
    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.try_fill_bytes(dst)
            .expect("CSPRNG fill_bytes failed irrecoverably")
    }

    /// 주어진 슬라이스를 암호학적으로 안전한 난수로 채우고, 실패를
    /// `Result` 로 보고합니다.
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<()>;
}

/// OS 엔트로피 직접 사용 RNG (무상태).
///
/// 매 호출마다 `elib-k0-nt/rng::os_entropy::fill_bytes` 를 호출해 OS 의
/// 보안 엔트로피 소스를 직접 사용합니다. 단일 호출당 32~64 바이트 수준의
/// 사용에는 충분히 빠르고, 이 정도 RNG 호출 빈도는 핸드셰이크 / ID 생성
/// 등 매 단위 동작당 1~2 회뿐입니다.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsRng;

impl Rng for OsRng {
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<()> {
        elib_rng::os_entropy::fill_bytes(dst).map_err(map_drbg_err)
    }
}

/// SHA-256 기반 Hash_DRBG 어댑터.
///
/// OS 엔트로피로 한번 시드한 뒤 [`Rng`] 인터페이스로 노출합니다. NIST
/// SP 800-90A 의 reseed 인터벌 (`2^48` 호출) 도달 시 자동으로 OS 엔트
/// 로피를 추가 수집해 reseed 합니다. 호출자는 이 동작을 신경 쓸 필요가
/// 없습니다.
pub struct HashDrbg(HashDRBGSHA256);

impl HashDrbg {
    /// OS 엔트로피로 새 DRBG 인스턴스를 생성합니다.
    ///
    /// `personalization` 은 "이 DRBG 인스턴스가 어떤 컨텍스트에서 사용
    /// 되는지" 를 나타내는 도메인 분리 라벨입니다 (예: `b"lumen.agent.v1"`).
    /// 동일 컨텍스트의 두 호출자가 다른 DRBG 출력을 받도록 보장합니다.
    pub fn from_os(personalization: Option<&[u8]>) -> Result<Self> {
        HashDRBGSHA256::new_from_os(personalization)
            .map(Self)
            .map_err(map_drbg_err)
    }
}

impl Rng for HashDrbg {
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<()> {
        // 단일 generate 호출당 최대 65536 바이트. lumen 호출자는 보통
        // 16~64 바이트만 채우므로 일반적으로 단일 호출로 끝납니다.
        let mut remaining = dst;
        while !remaining.is_empty() {
            let chunk = remaining.len().min(65_536);
            let (head, tail) = remaining.split_at_mut(chunk);
            match self.0.generate(head, None) {
                Ok(()) => {}
                Err(DrbgError::ReseedRequired) => {
                    let mut entropy = [0u8; 64];
                    elib_rng::os_entropy::fill_bytes(&mut entropy).map_err(map_drbg_err)?;
                    let reseed_res = self.0.reseed(&entropy, None);
                    // 엔트로피가 DRBG 내부 상태에 흡수되었으므로 스택 사본은
                    // 즉시 zeroize 합니다 - 실패 경로에서도 누설되지 않도록
                    // map_err 보다 먼저 수행합니다.
                    entropy.zeroize();
                    reseed_res.map_err(map_drbg_err)?;
                    self.0.generate(head, None).map_err(map_drbg_err)?;
                }
                Err(e) => return Err(map_drbg_err(e)),
            }
            remaining = tail;
        }
        Ok(())
    }
}

fn map_drbg_err(e: DrbgError) -> Error {
    Error::Crypto(format!("rng: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_rng_fills_bytes() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        OsRng.fill_bytes(&mut a);
        OsRng.fill_bytes(&mut b);
        assert_ne!(a, b, "OsRng must produce distinct outputs across calls");
        assert_ne!(a, [0u8; 32], "OsRng must not return all-zero buffer");
    }

    #[test]
    fn hash_drbg_fills_bytes() {
        let mut drbg = HashDrbg::from_os(Some(b"lumen.test.v1")).unwrap();
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        drbg.fill_bytes(&mut a);
        drbg.fill_bytes(&mut b);
        assert_ne!(a, b);
    }
}
