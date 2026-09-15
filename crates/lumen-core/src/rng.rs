//! Lumen의 RNG 추상화 모듈입니다.
//!
//! OS 엔트로피는 `getrandom` 크레이트, 시드형 DRBG 는 RustCrypto `chacha20`
//! 크레이트의 `ChaCha20Rng` 를 사용합니다. `rand` 전체 크레이트 대신 두
//! 검증된 최소 구성요소만 노출하는 얇은 래퍼입니다.
//!
//! # 트레이트와 구체 타입
//!
//! - [`Rng`] : 단일 메서드 `fill_bytes` 만 갖는 최소 트레이트. 임의의
//!   고정 길이 식별자 / 키 시드 / nonce 채움에 사용됩니다.
//! - [`OsRng`] : OS 엔트로피 (Linux `getrandom`, macOS / *BSD `getentropy`,
//!   Windows `ProcessPrng` 등) 를 직접 사용하는 무상태 RNG. 프로덕션 기본값.
//! - [`ChaChaDrbg`] : OS 엔트로피로 시드된 ChaCha20 기반 DRBG. 동일 페어가
//!   다량의 키 자료를 생성할 때 외부 entropy 호출 횟수를 줄이고 reseed
//!   인터벌을 명시적으로 제어합니다.

use chacha20::ChaCha20Rng;
use rand_core::{Rng as CoreRng, SeedableRng};
use zeroize::Zeroize;

use crate::error::{Error, Result};

/// 단일 책임의 RNG 트레이트입니다.
///
/// `rand_core::RngCore` 를 대체합니다. 호출자는 이 트레이트만 의존하면
/// 테스트에서는 결정론적 RNG 를, 프로덕션에서는 [`OsRng`] 를 주입할 수
/// 있습니다.
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
/// 매 호출마다 `getrandom::fill` 을 호출해 OS 의 보안 엔트로피 소스를 직접
/// 사용합니다. 단일 호출당 32~64 바이트 수준의 사용에는 충분히 빠르고, 이
/// 정도 RNG 호출 빈도는 핸드셰이크 / ID 생성 등 매 단위 동작당 1~2 회뿐입니다.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsRng;

impl Rng for OsRng {
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<()> {
        os_fill(dst)
    }
}

/// DRBG 시드 도출용 BLAKE3 도메인 분리 컨텍스트.
const DRBG_SEED_CONTEXT: &str = "lumen.rng.chacha20-drbg.seed.v1";

/// reseed 전 허용되는 최대 출력 바이트 수 (1 GiB).
const DRBG_RESEED_INTERVAL_BYTES: u64 = 1 << 30;

/// ChaCha20 기반 DRBG 어댑터.
///
/// OS 엔트로피 32 바이트와 personalization 라벨을 BLAKE3 `derive_key` 로
/// 결합해 시드를 만들고, `ChaCha20Rng` 로 출력합니다. 출력이
/// reseed 인터벌 (1 GiB) 에 도달하면 자동으로 OS 엔트로피를 다시
/// 수집해 reseed 합니다. 호출자는 이 동작을 신경 쓸 필요가 없습니다.
pub struct ChaChaDrbg {
    inner: ChaCha20Rng,
    personalization: Vec<u8>,
    generated: u64,
}

impl ChaChaDrbg {
    /// OS 엔트로피로 새 DRBG 인스턴스를 생성합니다.
    ///
    /// `personalization` 은 "이 DRBG 인스턴스가 어떤 컨텍스트에서 사용
    /// 되는지" 를 나타내는 도메인 분리 라벨입니다 (예: `b"lumen.agent.v1"`).
    /// 동일 컨텍스트의 두 호출자가 다른 DRBG 출력을 받도록 보장합니다.
    pub fn from_os(personalization: Option<&[u8]>) -> Result<Self> {
        let personalization = personalization.unwrap_or_default().to_vec();
        let inner = seeded_rng(&personalization)?;
        Ok(Self {
            inner,
            personalization,
            generated: 0,
        })
    }

    fn reseed(&mut self) -> Result<()> {
        self.inner = seeded_rng(&self.personalization)?;
        self.generated = 0;
        Ok(())
    }
}

impl Rng for ChaChaDrbg {
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<()> {
        let requested = dst.len() as u64;
        if self.generated.saturating_add(requested) > DRBG_RESEED_INTERVAL_BYTES {
            self.reseed()?;
        }
        self.inner.fill_bytes(dst);
        self.generated = self.generated.saturating_add(requested);
        Ok(())
    }
}

impl Drop for ChaChaDrbg {
    fn drop(&mut self) {
        self.personalization.zeroize();
    }
}

fn os_fill(dst: &mut [u8]) -> Result<()> {
    getrandom::fill(dst).map_err(|e| Error::Crypto(format!("rng: os entropy: {e}")))
}

fn seeded_rng(personalization: &[u8]) -> Result<ChaCha20Rng> {
    let mut entropy = [0u8; 32];
    os_fill(&mut entropy)?;
    let mut kdf = blake3::Hasher::new_derive_key(DRBG_SEED_CONTEXT);
    kdf.update(&entropy);
    kdf.update(personalization);
    let mut seed: [u8; 32] = *kdf.finalize().as_bytes();
    entropy.zeroize();
    let rng = ChaCha20Rng::from_seed(seed);
    seed.zeroize();
    Ok(rng)
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
    fn chacha_drbg_fills_bytes() {
        let mut drbg = ChaChaDrbg::from_os(Some(b"lumen.test.v1")).unwrap();
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        drbg.fill_bytes(&mut a);
        drbg.fill_bytes(&mut b);
        assert_ne!(a, b);
        assert_ne!(a, [0u8; 32]);
    }

    #[test]
    fn chacha_drbg_reseeds_after_interval() {
        let mut drbg = ChaChaDrbg::from_os(None).unwrap();
        drbg.generated = DRBG_RESEED_INTERVAL_BYTES - 8;
        let mut buf = [0u8; 16];
        drbg.fill_bytes(&mut buf);
        assert_eq!(drbg.generated, 16, "reseed must reset the output counter");
    }
}
