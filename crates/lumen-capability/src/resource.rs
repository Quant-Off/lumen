//! Capability 가 접근을 부여할 수 있는 리소스들.

use std::fmt;

use globset::{Glob, GlobMatcher};
use lumen_core::ToolId;
use serde::{Deserialize, Serialize};

/// 경로 glob 패턴 (예: `/var/data/**/*.parquet`).
#[derive(Clone, Debug)]
pub struct PathPattern {
    raw: String,
    matcher: GlobMatcher,
}

impl PathPattern {
    /// glob 패턴을 컴파일합니다.
    pub fn new(pattern: impl Into<String>) -> lumen_core::Result<Self> {
        let raw = pattern.into();
        let matcher = Glob::new(&raw)
            .map_err(|e| lumen_core::Error::Invalid(format!("bad path glob: {e}")))?
            .compile_matcher();
        Ok(Self { raw, matcher })
    }

    /// 경로를 이 패턴과 비교합니다.
    pub fn matches(&self, path: &std::path::Path) -> bool {
        self.matcher.is_match(path)
    }

    /// 원시 glob 문자열.
    pub fn as_raw(&self) -> &str {
        &self.raw
    }
}

impl PartialEq for PathPattern {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for PathPattern {}

impl Serialize for PathPattern {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for PathPattern {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// 네트워크 호스트 패턴 (literal host 또는 `*.example.com`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostPattern(pub String);

impl HostPattern {
    /// 호스트네임을 이 패턴과 비교합니다.
    pub fn matches(&self, host: &str) -> bool {
        if self.0 == "*" {
            return true;
        }
        if let Some(rest) = self.0.strip_prefix("*.") {
            return host.ends_with(rest) && host.len() > rest.len();
        }
        self.0 == host
    }
}

impl fmt::Display for HostPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Capability 가 접근을 허용하는 한 가지 리소스.
///
/// 리소스는 단일 타깃이 아닌 *패턴* 입니다 - 단일 capability 가 디렉토리
/// 아래 여러 파일 읽기를 한꺼번에 허용할 수 있습니다.
///
/// 정규 직렬화 페이로드는 `postcard` (자기 기술적이지 않음) 로 인코딩되어
/// internally / adjacently tagged enum 을 거부하므로 외부 태깅 (기본) 을
/// 사용합니다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    /// 파일이나 디렉토리 트리 읽기.
    FsRead(PathPattern),
    /// 파일이나 디렉토리 트리 쓰기.
    FsWrite(PathPattern),
    /// 호스트로의 outbound 연결 열기.
    Net(HostPattern),
    /// 등록된 도구 호출.
    Tool(ToolId),
    /// 최대 `max` 토큰까지 추론 사용.
    InferenceTokens {
        /// 토큰 상한.
        max: u32,
    },
    /// ZK 증명 생성 요청.
    ZkProofRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_wildcard() {
        let p = HostPattern("*.example.com".into());
        assert!(p.matches("a.example.com"));
        assert!(p.matches("b.c.example.com"));
        assert!(!p.matches("example.com"));
        assert!(!p.matches("example.org"));
    }

    #[test]
    fn host_literal() {
        let p = HostPattern("api.lumen.dev".into());
        assert!(p.matches("api.lumen.dev"));
        assert!(!p.matches("evil.com"));
    }

    #[test]
    fn path_glob() {
        let p = PathPattern::new("/var/data/**/*.parquet").unwrap();
        assert!(p.matches(std::path::Path::new("/var/data/x/y.parquet")));
        assert!(!p.matches(std::path::Path::new("/etc/passwd")));
    }

    #[test]
    fn resource_serde_human() {
        let r = Resource::Tool(ToolId::new("echo").unwrap());
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"Tool\""));
        let back: Resource = serde_json::from_str(&j).unwrap();
        assert_eq!(back, r);
    }
}
