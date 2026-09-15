//! 최소 HTTP/1.1 클라이언트 (요청 1 회 = 연결 1 회).
//!
//! 서버형 추론 엔진 (llama-server, 향후 OpenAI 호환 서버) 은 로컬 소켓 위
//! HTTP + JSON / SSE 로 대화합니다. `reqwest` / `hyper` 는 TLS 스택과 큰
//! 의존 트리를 끌어오므로 폐쇄망 정책상 채택하지 않고, 필요한 부분집합만
//! 직접 구현합니다.
//!
//! # 지원 범위
//! - 요청: `GET` / `POST`, 고정 헤더 + `Content-Length` 바디, `Connection: close`.
//! - 응답: 상태 줄, 헤더, `Content-Length` / `Transfer-Encoding: chunked` /
//!   연결 종료까지 읽기 바디.
//! - SSE (`text/event-stream`) 프레임 분리 (`data:` 필드만).
//!
//! # 자원 상한
//! 헤더 [`MAX_HEADER_BYTES`], 바디 [`MAX_BODY_BYTES`] 를 초과하면 즉시
//! 에러입니다. 신뢰할 수 없는 peer 가 메모리를 고갈시키지 못하게 합니다.
//! 소켓 자체는 로컬 (UDS / loopback) 이지만 제로 트러스트 원칙상 peer 의
//! 응답도 검증 대상입니다.

use std::pin::Pin;

use lumen_core::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// 응답 헤더 블록 최대 크기.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// 비스트리밍 응답 바디 최대 크기.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
/// SSE 단일 이벤트 최대 크기.
pub const MAX_SSE_EVENT_BYTES: usize = 4 * 1024 * 1024;

/// 바이트 스트림 trait 객체.
pub type IoStream = Pin<Box<dyn AsyncStream>>;

/// `AsyncRead + AsyncWrite + Send + Unpin` 의 별칭 trait.
pub trait AsyncStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncStream for T {}

/// 발신 요청.
#[derive(Clone, Debug)]
pub struct Request {
    /// `GET` 또는 `POST`.
    pub method: &'static str,
    /// `/completion` 등 경로 (쿼리 포함 가능).
    pub path: String,
    /// 추가 헤더 (`Authorization` 등).
    pub headers: Vec<(String, String)>,
    /// 바디 (POST 인 경우).
    pub body: Vec<u8>,
}

impl Request {
    /// GET 요청.
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: "GET",
            path: path.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// JSON 바디 POST 요청.
    pub fn post_json(path: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method: "POST",
            path: path.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body,
        }
    }

    /// 헤더 추가 (빌더).
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn serialize(&self) -> Result<Vec<u8>> {
        if self.path.is_empty()
            || !self.path.starts_with('/')
            || self.path.contains(char::is_whitespace)
        {
            return Err(Error::Invalid(format!("http: bad path {:?}", self.path)));
        }
        let mut out = Vec::with_capacity(256 + self.body.len());
        out.extend_from_slice(self.method.as_bytes());
        out.push(b' ');
        out.extend_from_slice(self.path.as_bytes());
        out.extend_from_slice(b" HTTP/1.1\r\nHost: lumen\r\nConnection: close\r\nAccept: */*\r\n");
        for (k, v) in &self.headers {
            if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
                return Err(Error::Invalid("http: header injection rejected".into()));
            }
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        if self.method == "POST" || !self.body.is_empty() {
            out.extend_from_slice(format!("Content-Length: {}\r\n", self.body.len()).as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        Ok(out)
    }
}

/// 바디 프레이밍 모드.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Framing {
    Length(usize),
    Chunked,
    UntilClose,
}

/// 응답 헤더와 바디 리더.
pub struct Response {
    /// HTTP 상태 코드.
    pub status: u16,
    /// 소문자 이름의 헤더 목록.
    pub headers: Vec<(String, String)>,
    body: Body,
}

impl Response {
    /// 헤더 값 조회 (이름은 대소문자 무시).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 바디 전체를 [`MAX_BODY_BYTES`] 상한으로 읽습니다.
    pub async fn read_body(mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = self.body.read_some(&mut buf).await?;
            if n == 0 {
                break;
            }
            if out.len() + n > MAX_BODY_BYTES {
                return Err(Error::Inference("http: response body exceeds limit".into()));
            }
            out.extend_from_slice(&buf[..n]);
        }
        Ok(out)
    }

    /// 바디를 SSE 이벤트 리더로 전환합니다.
    pub fn into_sse(self) -> SseReader {
        SseReader {
            body: self.body,
            pending: Vec::new(),
            eof: false,
        }
    }
}

struct Body {
    io: IoStream,
    /// 헤더 파싱 시 함께 읽힌 바디 선두.
    prefetched: Vec<u8>,
    framing: Framing,
    /// Length 모드: 남은 바이트. Chunked 모드: 현재 청크의 남은 바이트.
    remaining: usize,
    done: bool,
}

impl Body {
    /// 프레이밍을 해석한 바디 바이트를 `buf` 에 채웁니다. `0` 이면 종료.
    async fn read_some(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.done {
            return Ok(0);
        }
        match self.framing {
            Framing::Length(_) => {
                if self.remaining == 0 {
                    self.done = true;
                    return Ok(0);
                }
                let want = buf.len().min(self.remaining);
                let n = self.raw_read(&mut buf[..want]).await?;
                if n == 0 {
                    return Err(Error::Inference(
                        "http: connection closed before Content-Length satisfied".into(),
                    ));
                }
                self.remaining -= n;
                if self.remaining == 0 {
                    self.done = true;
                }
                Ok(n)
            }
            Framing::UntilClose => {
                let n = self.raw_read(buf).await?;
                if n == 0 {
                    self.done = true;
                }
                Ok(n)
            }
            Framing::Chunked => {
                if self.remaining == 0 {
                    // 청크 크기 줄 읽기.
                    let line = self.raw_read_line().await?;
                    let size_str = line.split(';').next().unwrap_or("").trim();
                    let size = usize::from_str_radix(size_str, 16).map_err(|_| {
                        Error::Inference(format!("http: bad chunk size {size_str:?}"))
                    })?;
                    if size == 0 {
                        // 트레일러 스킵.
                        loop {
                            let l = self.raw_read_line().await?;
                            if l.is_empty() {
                                break;
                            }
                        }
                        self.done = true;
                        return Ok(0);
                    }
                    self.remaining = size;
                }
                let want = buf.len().min(self.remaining);
                let n = self.raw_read(&mut buf[..want]).await?;
                if n == 0 {
                    return Err(Error::Inference("http: connection closed mid-chunk".into()));
                }
                self.remaining -= n;
                if self.remaining == 0 {
                    // 청크 끝의 CRLF.
                    let crlf = self.raw_read_line().await?;
                    if !crlf.is_empty() {
                        return Err(Error::Inference("http: missing chunk CRLF".into()));
                    }
                }
                Ok(n)
            }
        }
    }

    async fn raw_read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if !self.prefetched.is_empty() {
            let n = buf.len().min(self.prefetched.len());
            buf[..n].copy_from_slice(&self.prefetched[..n]);
            self.prefetched.drain(..n);
            return Ok(n);
        }
        Ok(self.io.read(buf).await?)
    }

    /// CRLF (또는 LF) 로 끝나는 한 줄을 읽어 개행 없이 반환합니다.
    async fn raw_read_line(&mut self) -> Result<String> {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = self.raw_read(&mut byte).await?;
            if n == 0 {
                return Err(Error::Inference("http: eof inside chunk framing".into()));
            }
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
            if line.len() > 256 {
                return Err(Error::Inference("http: chunk line too long".into()));
            }
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        String::from_utf8(line).map_err(|_| Error::Inference("http: chunk line not utf-8".into()))
    }
}

/// SSE `data:` 이벤트 리더.
pub struct SseReader {
    body: Body,
    pending: Vec<u8>,
    eof: bool,
}

impl SseReader {
    /// 다음 이벤트의 `data` 페이로드 (여러 `data:` 줄은 `\n` 으로 결합).
    /// 스트림 종료 시 `None`.
    pub async fn next_data(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(event) = self.take_event()? {
                if let Some(data) = extract_data(&event) {
                    return Ok(Some(data));
                }
                continue; // 주석 / 다른 필드만 있는 이벤트.
            }
            if self.eof {
                // 종결 구분자 없이 남은 데이터.
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let rest = std::mem::take(&mut self.pending);
                let rest = String::from_utf8_lossy(&rest).into_owned();
                return Ok(extract_data(&rest));
            }
            let mut buf = [0u8; 8192];
            let n = self.body.read_some(&mut buf).await?;
            if n == 0 {
                self.eof = true;
                continue;
            }
            if self.pending.len() + n > MAX_SSE_EVENT_BYTES {
                return Err(Error::Inference("http: sse event exceeds limit".into()));
            }
            self.pending.extend_from_slice(&buf[..n]);
        }
    }

    fn take_event(&mut self) -> Result<Option<String>> {
        // 이벤트 구분자: 빈 줄 ("\n\n" 또는 "\r\n\r\n").
        let sep = find_event_separator(&self.pending);
        let Some((pos, len)) = sep else {
            return Ok(None);
        };
        let event: Vec<u8> = self.pending.drain(..pos).collect();
        self.pending.drain(..len);
        Ok(Some(String::from_utf8(event).map_err(|_| {
            Error::Inference("http: sse event not utf-8".into())
        })?))
    }
}

fn find_event_separator(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n");
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) if b < a => Some((b, 4)),
        (Some(a), _) => Some((a, 2)),
        (None, Some(b)) => Some((b, 4)),
        (None, None) => None,
    }
}

fn extract_data(event: &str) -> Option<String> {
    let mut data: Vec<&str> = Vec::new();
    for line in event.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

/// 요청을 보내고 응답 헤더까지 파싱합니다. 바디는 [`Response`] 로 지연
/// 읽기합니다.
pub async fn send(mut io: IoStream, req: &Request) -> Result<Response> {
    let bytes = req.serialize()?;
    io.write_all(&bytes).await?;
    io.flush().await?;

    // 헤더 블록 읽기.
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 2048];
    let split_at = loop {
        if let Some(pos) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if head.len() > MAX_HEADER_BYTES {
            return Err(Error::Inference(
                "http: response headers exceed limit".into(),
            ));
        }
        let n = io.read(&mut buf).await?;
        if n == 0 {
            return Err(Error::Inference(
                "http: connection closed before headers".into(),
            ));
        }
        head.extend_from_slice(&buf[..n]);
    };
    let prefetched = head.split_off(split_at + 4);
    head.truncate(split_at);
    let head = std::str::from_utf8(&head)
        .map_err(|_| Error::Inference("http: headers not utf-8".into()))?;

    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(Error::Inference(format!(
            "http: bad status line {status_line:?}"
        )));
    }
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::Inference(format!("http: bad status line {status_line:?}")))?;

    let mut headers = Vec::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
    }

    let framing = if headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"))
    {
        Framing::Chunked
    } else if let Some((_, v)) = headers.iter().find(|(k, _)| k == "content-length") {
        let len: usize = v
            .parse()
            .map_err(|_| Error::Inference(format!("http: bad content-length {v:?}")))?;
        if len > MAX_BODY_BYTES {
            return Err(Error::Inference(
                "http: content-length exceeds limit".into(),
            ));
        }
        Framing::Length(len)
    } else {
        Framing::UntilClose
    };
    let remaining = match framing {
        Framing::Length(n) => n,
        _ => 0,
    };

    Ok(Response {
        status,
        headers,
        body: Body {
            io,
            prefetched,
            framing,
            remaining,
            done: matches!(framing, Framing::Length(0)),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn serve(raw: &'static [u8], req: Request) -> Result<Response> {
        let (client, mut server) = duplex(64);
        tokio::spawn(async move {
            let mut sink = vec![0u8; 4096];
            // 요청을 소비 (헤더 끝까지).
            let mut got = Vec::new();
            loop {
                let n = server.read(&mut sink).await.unwrap();
                got.extend_from_slice(&sink[..n]);
                if got.windows(4).any(|w| w == b"\r\n\r\n") || n == 0 {
                    break;
                }
            }
            // 일부러 작은 조각으로 나눠 씀 (경계 처리 검증).
            for chunk in raw.chunks(7) {
                server.write_all(chunk).await.unwrap();
            }
            server.shutdown().await.unwrap();
        });
        send(Box::pin(client), &req).await
    }

    #[tokio::test]
    async fn content_length_body() {
        let resp = serve(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\nhello world",
            Request::get("/x"),
        )
        .await
        .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.header("content-type"), Some("application/json"));
        assert_eq!(resp.read_body().await.unwrap(), b"hello world");
    }

    #[tokio::test]
    async fn chunked_body_across_boundaries() {
        let resp = serve(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nX-Trailer: 1\r\n\r\n",
            Request::post_json("/y", b"{}".to_vec()),
        )
        .await
        .unwrap();
        assert_eq!(resp.read_body().await.unwrap(), b"hello world");
    }

    #[tokio::test]
    async fn until_close_body() {
        let resp = serve(
            b"HTTP/1.1 503 Service Unavailable\r\n\r\nloading",
            Request::get("/health"),
        )
        .await
        .unwrap();
        assert_eq!(resp.status, 503);
        assert_eq!(resp.read_body().await.unwrap(), b"loading");
    }

    #[tokio::test]
    async fn sse_events_split_and_joined() {
        let resp = serve(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n\
              2b\r\n: comment\n\ndata: {\"a\":1}\n\ndata: x\ndata: y\n\n\r\n0\r\n\r\n",
            Request::get("/s"),
        )
        .await
        .unwrap();
        let mut sse = resp.into_sse();
        assert_eq!(sse.next_data().await.unwrap().as_deref(), Some("{\"a\":1}"));
        assert_eq!(sse.next_data().await.unwrap().as_deref(), Some("x\ny"));
        assert_eq!(sse.next_data().await.unwrap(), None);
    }

    #[tokio::test]
    async fn truncated_content_length_errors() {
        let resp = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\nshort",
            Request::get("/"),
        )
        .await
        .unwrap();
        assert!(resp.read_body().await.is_err());
    }

    #[tokio::test]
    async fn header_injection_rejected() {
        let req = Request::get("/").header("X-Bad", "a\r\nInjected: 1");
        assert!(req.serialize().is_err());
        assert!(Request::get("no-slash").serialize().is_err());
        assert!(Request::get("/a b").serialize().is_err());
    }

    #[test]
    fn request_wire_format() {
        let req = Request::post_json("/completion", b"{\"a\":1}".to_vec())
            .header("Authorization", "Bearer k");
        let s = String::from_utf8(req.serialize().unwrap()).unwrap();
        assert!(s.starts_with("POST /completion HTTP/1.1\r\nHost: lumen\r\nConnection: close\r\n"));
        assert!(s.contains("Authorization: Bearer k\r\n"));
        assert!(s.ends_with("Content-Length: 7\r\n\r\n{\"a\":1}"));
    }
}
