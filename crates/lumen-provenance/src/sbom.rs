//! Lumen 이 관리하는 모델 산출물용 미니멀 CycloneDX 1.5 SBOM emitter.
//!
//! 출력은 CycloneDX 스키마의 필수 필드를 준수하는 JSON 입니다. 다운스트림
//! 도구 (Dependency-Track, Trivy, Anchore, …) 가 받아들일 수 있는 가장
//! 작은 그럴듯한 문서를 발행합니다. 전체 스키마 충실도는 v0 범위 밖.

use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::manifest::ModelManifest;

/// CycloneDX 최상위 문서.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomDocument {
    /// 항상 `"CycloneDX"`.
    #[serde(rename = "bomFormat")]
    pub bom_format: String,
    /// CycloneDX 스키마 버전.
    #[serde(rename = "specVersion")]
    pub spec_version: String,
    /// 이 문서의 `urn:uuid:` 식별자.
    #[serde(rename = "serialNumber")]
    pub serial_number: String,
    /// CycloneDX BOM 버전 (첫 발행 시 1).
    pub version: u32,
    /// 컴포넌트별 메타데이터.
    pub metadata: SbomMetadata,
    /// 소프트웨어 및 ML 컴포넌트.
    pub components: Vec<SbomComponent>,
}

/// 문서 수준 메타데이터.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomMetadata {
    /// ISO-8601 UTC timestamp.
    pub timestamp: String,
    /// 이 SBOM 을 발행한 도구.
    pub tools: Vec<SbomTool>,
}

/// 발행 도구 항목.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomTool {
    /// 벤더 / 프로젝트 이름.
    pub vendor: String,
    /// 도구 이름.
    pub name: String,
    /// 도구 버전.
    pub version: String,
}

/// 컴포넌트 한 개 (우리 경우는 모델 산출물).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomComponent {
    /// CycloneDX 컴포넌트 타입. `machine-learning-model` 사용.
    #[serde(rename = "type")]
    pub component_type: String,
    /// 상호 참조용 BOM-ref.
    #[serde(rename = "bom-ref")]
    pub bom_ref: String,
    /// 컴포넌트 이름.
    pub name: String,
    /// 컴포넌트 버전.
    pub version: String,
    /// 암호학적 해시.
    pub hashes: Vec<SbomHash>,
    /// SPDX 라이선스.
    pub licenses: Vec<SbomLicense>,
}

/// CycloneDX 형식의 단일 해시.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomHash {
    /// 알고리즘. `BLAKE3` 사용.
    pub alg: String,
    /// 소문자 hex 해시 콘텐츠.
    pub content: String,
}

/// 단일 라이선스 항목.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomLicense {
    /// 라이선스 하위 문서.
    pub license: SbomLicenseId,
}

/// SPDX 라이선스 식별자.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SbomLicenseId {
    /// SPDX 표현식.
    pub id: String,
}

/// 매니페스트들로부터 CycloneDX SBOM 문서를 빌드합니다.
pub fn generate_sbom(manifests: &[ModelManifest]) -> Result<SbomDocument> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| Error::Provenance(format!("clock: {e}")))?;
    let secs = now.as_secs();
    let timestamp = format_iso8601(secs);

    let serial_number = format!("urn:uuid:{}", pseudo_uuid(secs));

    let components = manifests
        .iter()
        .map(|m| SbomComponent {
            component_type: "machine-learning-model".into(),
            bom_ref: format!("model:{}@{}", m.name, m.version),
            name: m.name.clone(),
            version: m.version.clone(),
            hashes: vec![SbomHash {
                alg: "BLAKE3".into(),
                content: m.hash.to_hex(),
            }],
            licenses: m
                .license
                .iter()
                .map(|id| SbomLicense {
                    license: SbomLicenseId { id: id.clone() },
                })
                .collect(),
        })
        .collect();

    Ok(SbomDocument {
        bom_format: "CycloneDX".into(),
        spec_version: "1.5".into(),
        serial_number,
        version: 1,
        metadata: SbomMetadata {
            timestamp,
            tools: vec![SbomTool {
                vendor: "lumen".into(),
                name: "lumen-provenance".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            }],
        },
        components,
    })
}

fn format_iso8601(secs: u64) -> String {
    // Howard Hinnant 의 알고리즘으로 epoch 이후 일수를 날짜로 변환하는
    // 간단한 "YYYY-MM-DDThh:mm:ssZ" 포맷터. 일회성 SBOM 발행에
    // `chrono`/`time` 을 끌어오지 않기 위함입니다.
    let days = (secs / 86_400) as i64;
    let mut h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    let _ = &mut h;

    let z = days + 719_468;
    let era = if z >= 0 { z / 146_097 } else { (z - 146_096) / 146_097 };
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mm = if mp < 10 { mp + 3 } else { mp - 9 };
    let yy = y + i64::from(mm <= 2);

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yy, mm, d, h, m, s)
}

fn pseudo_uuid(seed: u64) -> String {
    // 진짜 UUIDv4 가 아닙니다. CycloneDX 는 "urn:uuid:" 형식을 요구하지만
    // 무작위성을 검증하지는 않습니다. v0 에는 충분 - 문서 해시가 어차피
    // 이를 매니페스트 집합에 묶습니다.
    let a = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = seed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (a >> 32) as u32,
        (a >> 16) as u16,
        a as u16,
        (b >> 48) as u16,
        b & 0xFFFF_FFFF_FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::Blake3Hash;
    use std::path::PathBuf;

    use crate::manifest::Format;

    #[test]
    fn sbom_has_required_fields() {
        let m = ModelManifest {
            name: "tiny".into(),
            version: "0.0.1".into(),
            path: PathBuf::from("models/tiny.safetensors"),
            format: Format::Safetensors,
            hash: Blake3Hash::of(b"tiny"),
            license: Some("Apache-2.0".into()),
            signature: None,
            signer: None,
        };
        let doc = generate_sbom(&[m]).unwrap();
        assert_eq!(doc.bom_format, "CycloneDX");
        assert_eq!(doc.spec_version, "1.5");
        assert_eq!(doc.components.len(), 1);
        assert_eq!(doc.components[0].hashes[0].alg, "BLAKE3");
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("\"BLAKE3\""));
        assert!(json.contains("\"CycloneDX\""));
    }
}
