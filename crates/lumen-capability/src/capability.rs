//! 서명된 Capability 토큰.

use lumen_core::{
    AgentId, CapabilityId, Error, Result, Signature, SigningKey, Timestamp, VerifyingKey,
};
use serde::{Deserialize, Serialize};

use crate::resource::Resource;

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
        let bytes = postcard::to_allocvec(&body)
            .map_err(|e| Error::Decode(format!("capability body encode: {e}")))?;
        let signature = key.sign(&bytes);
        Ok(Self { body, signature })
    }

    /// detached 서명을 `key` 로 검증합니다.
    ///
    /// 만료, audience, 리플레이는 **검사하지 않습니다** - 그것은
    /// [`crate::PolicyEngine`] 의 책임입니다.
    pub fn verify_signature(&self, key: &VerifyingKey) -> Result<()> {
        let bytes = postcard::to_allocvec(&self.body)
            .map_err(|e| Error::Decode(format!("capability body encode: {e}")))?;
        key.verify(&bytes, &self.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ToolId;
    use rand::rngs::OsRng;

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
}
