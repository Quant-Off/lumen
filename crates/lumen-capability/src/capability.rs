//! 서명된 Capability 토큰.

use lumen_core::{
    AgentId, CapabilityId, Error, Result, Signature, SigningKey, Timestamp, VerifyingKey,
};
use serde::{Deserialize, Serialize};

use crate::resource::Resource;

/// Ed25519 도메인 분리 prefix.
///
/// Lumen 의 다른 서명 페이로드 (보안 채널 핸드셰이크 / 데이터 프레임 / 모델
/// 매니페스트) 와 동일한 신원 키를 공유하더라도, 이 prefix 가 서명 입력 첫
/// 부분에 포함되므로 다른 프로토콜의 정상 서명을 capability 검증에 재사용
/// 하는 cross-protocol replay 가 차단됩니다.
const CAPABILITY_SIGN_DOMAIN: &[u8] = b"lumen.capability.body.v1";

/// Capability 의 본문 - 서명되는 모든 데이터.
///
/// 본문과 서명을 분리해 두면 동일한 정규 표현을 서명과 검증 양쪽에서
/// 재사용할 수 있어 모호성이 사라집니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityBody {
    /// audit/log 상관관계용 고유 식별자.
    pub id: CapabilityId,
    /// 이 capability 를 휘두를 수 있는 에이전트.
    pub audience: AgentId,
    /// 부여되는 리소스.
    pub resource: Resource,
    /// 리플레이 방지를 위한 16 바이트 난수.
    pub nonce: [u8; 16],
    /// 이 시각 이후로는 capability 가 무효.
    pub expires_at: Timestamp,
    /// Issuer 의 공개 키 fingerprint (빠른 trust-anchor lookup 용).
    pub issuer: AgentId,
}

/// Capability 토큰: 서명된 본문 + detached 서명.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// 서명된 내부 페이로드.
    pub body: CapabilityBody,
    /// `postcard(body)` 위에 작성된 detached Ed25519 서명.
    pub signature: Signature,
}

impl Capability {
    /// 주어진 서명 키로 본문에 서명해 완성된 capability 를 만듭니다.
    pub fn sign(body: CapabilityBody, key: &SigningKey) -> Result<Self> {
        let payload = signing_payload(&body)?;
        let signature = key.sign(&payload);
        Ok(Self { body, signature })
    }

    /// detached 서명을 `key` 로 검증합니다.
    ///
    /// 만료, audience, 리플레이는 **검사하지 않습니다** - 그것은
    /// [`crate::PolicyEngine`] 의 책임입니다.
    pub fn verify_signature(&self, key: &VerifyingKey) -> Result<()> {
        let payload = signing_payload(&self.body)?;
        key.verify(&payload, &self.signature)
    }
}

/// `(CAPABILITY_SIGN_DOMAIN || postcard(body))` 를 정규 서명 페이로드로 반환.
fn signing_payload(body: &CapabilityBody) -> Result<Vec<u8>> {
    let body_bytes = postcard::to_allocvec(body)
        .map_err(|e| Error::Decode(format!("capability body encode: {e}")))?;
    let mut out = Vec::with_capacity(CAPABILITY_SIGN_DOMAIN.len() + body_bytes.len());
    out.extend_from_slice(CAPABILITY_SIGN_DOMAIN);
    out.extend_from_slice(&body_bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::rng::OsRng;
    use lumen_core::ToolId;

    use crate::Resource;

    #[test]
    fn sign_verify_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let issuer = AgentId::random(&mut OsRng);
        let audience = AgentId::random(&mut OsRng);
        let body = CapabilityBody {
            id: CapabilityId::random(&mut OsRng),
            audience,
            resource: Resource::Tool(ToolId::new("echo").unwrap()),
            nonce: [7u8; 16],
            expires_at: Timestamp::FOREVER,
            issuer,
        };
        let cap = Capability::sign(body.clone(), &sk).unwrap();
        cap.verify_signature(&vk).unwrap();

        // 변조: resource 를 바꾼 뒤 재검증; 실패해야 합니다.
        let mut tampered = cap.clone();
        tampered.body.resource = Resource::ZkProofRequest;
        assert!(tampered.verify_signature(&vk).is_err());
    }

    /// 도메인 분리 검증: domain prefix 가 없는 직접 postcard 서명은
    /// `verify_signature` 에서 거부되어야 합니다. cross-protocol replay
    /// 보호의 회귀 테스트입니다.
    #[test]
    fn rejects_signature_without_domain_prefix() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let body = CapabilityBody {
            id: CapabilityId::random(&mut OsRng),
            audience: AgentId::random(&mut OsRng),
            resource: Resource::Tool(ToolId::new("echo").unwrap()),
            nonce: [3u8; 16],
            expires_at: Timestamp::FOREVER,
            issuer: AgentId::random(&mut OsRng),
        };
        // 공격자가 도메인 prefix 없이 postcard(body) 만 서명 - 다른 프로토콜의
        // 정상 서명을 capability 서명으로 재사용하는 시나리오.
        let raw = postcard::to_allocvec(&body).unwrap();
        let bad_sig = sk.sign(&raw);
        let bad_cap = Capability {
            body,
            signature: bad_sig,
        };
        assert!(
            bad_cap.verify_signature(&vk).is_err(),
            "domain prefix 없는 서명은 거부되어야 함"
        );
    }
}
