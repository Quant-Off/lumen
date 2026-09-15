//! 채널 기반 추론 엔진과 TEE peer 루프.
//!
//! 모든 completion 요청을 [`lumen_channel::SecureChannel`] 로 forward 하고
//! 응답을 읽어옵니다. 반대편은 호스트 TEE 의 Lumen peer ([`run_peer_with`])
//! 이며, peer 는 자신이 가진 어떤 백엔드 ([`crate::Engines`], 예:
//! `LlamaServerEngine`) 로든 요청을 처리합니다. 즉 샌드박스 쪽 코드는 어떤
//! 엔진이 뒤에 있는지 알 필요가 없습니다.
//!
//! # 와이어 프로토콜 (`lumen.infer.v2`)
//!
//! ```text
//! client -> peer : InferRequest { version, prompt, params, stream }
//! peer -> client : InferFrame::Done(Completion)              (stream = false)
//!                  InferFrame::Token* , InferFrame::Done      (stream = true)
//!                  InferFrame::Error(String)                   (양쪽 모두)
//! ```
//!
//! postcard 는 자기 기술적이지 않으므로 `version` 필드로 호환성을 명시
//! 합니다. peer 는 다른 버전을 [`InferFrame::Error`] 로 거부합니다.
//!
//! 스트리밍 도중 클라이언트가 스트림을 drop 하면, 남은 프레임을 백그라운드
//! 에서 소진해 채널 상태를 동기화한 뒤 잠금을 해제합니다.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use lumen_channel::SecureChannel;
use lumen_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::streaming::{FinishReason, StreamingEngine, Token, TokenStream};
use crate::{Completion, EngineCapabilities, EngineInfo, Engines, InferenceEngine, SamplingParams};

/// 와이어 프로토콜 버전.
pub const WIRE_VERSION: u16 = 2;

/// 와이어 형식 요청.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferRequest {
    /// [`WIRE_VERSION`].
    pub version: u16,
    /// 프롬프트 텍스트.
    pub prompt: String,
    /// sampling 컨트롤.
    pub params: SamplingParams,
    /// 토큰 단위 스트리밍 요청 여부.
    pub stream: bool,
}

/// peer -> client 프레임.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InferFrame {
    /// 스트리밍 토큰.
    Token(Token),
    /// 최종 completion.
    Done(Completion),
    /// 에러 (요청 종료).
    Error(String),
}

/// 호출을 채널로 forward 하는 엔진.
///
/// 채널은 `Arc<Mutex<_>>` 로 보유해 스트리밍 중에도 소유 가드를 스트림에
/// 실어 보낼 수 있습니다.
pub struct ChannelEngine<C: SecureChannel> {
    channel: Arc<Mutex<C>>,
}

impl<C: SecureChannel> ChannelEngine<C> {
    /// 채널을 wrap.
    pub fn new(channel: C) -> Self {
        Self {
            channel: Arc::new(Mutex::new(channel)),
        }
    }
}

#[async_trait]
impl<C: SecureChannel + 'static> InferenceEngine for ChannelEngine<C> {
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion> {
        let req = InferRequest {
            version: WIRE_VERSION,
            prompt: prompt.to_string(),
            params: params.clone(),
            stream: false,
        };
        let mut guard = self.channel.lock().await;
        guard.send(&req).await?;
        loop {
            match guard.recv::<InferFrame>().await? {
                InferFrame::Done(c) => return Ok(c),
                InferFrame::Error(e) => return Err(Error::Inference(e)),
                // 비스트리밍 요청에 토큰이 오면 무시하고 Done 을 기다립니다.
                InferFrame::Token(_) => continue,
            }
        }
    }

    fn info(&self) -> EngineInfo {
        EngineInfo {
            backend: "channel".into(),
            model: None,
            capabilities: EngineCapabilities {
                streaming: true,
                ..Default::default()
            },
            max_context: None,
        }
    }
}

/// 스트림 drop 시 남은 프레임을 소진하는 가드.
struct StreamGuard<C: SecureChannel + 'static> {
    guard: Option<OwnedMutexGuard<C>>,
    finished: bool,
}

impl<C: SecureChannel + 'static> Drop for StreamGuard<C> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let Some(mut guard) = self.guard.take() else {
            return;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    while let Ok(InferFrame::Token(_)) = guard.recv::<InferFrame>().await {}
                });
            }
            Err(_) => {
                tracing::warn!(
                    target: "lumen.inference.channel",
                    "stream dropped outside a tokio runtime; channel may be desynchronized"
                );
            }
        }
    }
}

#[async_trait]
impl<C: SecureChannel + 'static> StreamingEngine for ChannelEngine<C> {
    async fn stream_complete(&self, prompt: &str, params: &SamplingParams) -> Result<TokenStream> {
        let req = InferRequest {
            version: WIRE_VERSION,
            prompt: prompt.to_string(),
            params: params.clone(),
            stream: true,
        };
        let mut guard = self.channel.clone().lock_owned().await;
        guard.send(&req).await?;
        let state = StreamGuard {
            guard: Some(guard),
            finished: false,
        };
        let s = stream::unfold(state, |mut st| async move {
            if st.finished {
                return None;
            }
            let guard = st.guard.as_mut()?;
            match guard.recv::<InferFrame>().await {
                Ok(InferFrame::Token(t)) => {
                    if t.finish_reason.is_some() {
                        // peer 는 마지막 토큰 뒤에 Done 을 보냅니다. 소진합니다.
                        let _ = guard.recv::<InferFrame>().await;
                        st.finished = true;
                    }
                    Some((Ok(t), st))
                }
                Ok(InferFrame::Done(_)) => {
                    st.finished = true;
                    // finish_reason 없는 토큰 뒤 Done 만 온 경우 종료 토큰 합성.
                    Some((
                        Ok(Token {
                            id: None,
                            text: String::new(),
                            logprob: None,
                            finish_reason: Some(FinishReason::Eos),
                        }),
                        st,
                    ))
                }
                Ok(InferFrame::Error(e)) => {
                    st.finished = true;
                    Some((Err(Error::Inference(e)), st))
                }
                Err(e) => {
                    st.finished = true;
                    Some((Err(e), st))
                }
            }
        });
        Ok(Box::pin(s))
    }
}

/// `channel` 위에서 inference 요청을 처리하는 "TEE peer" 루프 (비스트리밍
/// 엔진 전용 편의 함수). 스트리밍 요청은 `complete` 로 처리한 뒤 `Done` 만
/// 보냅니다.
pub async fn run_peer<C: SecureChannel, E: InferenceEngine>(channel: C, engine: &E) -> Result<()> {
    run_peer_inner(channel, engine, None).await
}

/// `channel` 위에서 inference 요청을 [`Engines`] 로 처리하는 TEE peer 루프.
///
/// 클라이언트가 스트리밍을 요청했고 `engines.streaming` 이 있으면 토큰
/// 프레임을 실시간으로 forward 한 뒤 누적 텍스트로 `Done` 을 보냅니다.
/// 스트리밍 엔진이 없으면 `complete` 결과를 `Done` 하나로 보냅니다.
pub async fn run_peer_with<C: SecureChannel>(channel: C, engines: &Engines) -> Result<()> {
    run_peer_inner(
        channel,
        engines.inference.as_ref(),
        engines.streaming.as_deref(),
    )
    .await
}

async fn run_peer_inner<C: SecureChannel>(
    mut channel: C,
    inference: &dyn InferenceEngine,
    streaming: Option<&dyn StreamingEngine>,
) -> Result<()> {
    loop {
        let req: InferRequest = match channel.recv().await {
            Ok(r) => r,
            Err(Error::Channel(_)) => return Ok(()), // peer 종료
            Err(e) => return Err(e),
        };
        if req.version != WIRE_VERSION {
            channel
                .send(&InferFrame::Error(format!(
                    "unsupported infer wire version {} (peer speaks {WIRE_VERSION})",
                    req.version
                )))
                .await?;
            continue;
        }

        match (req.stream, streaming) {
            (true, Some(streaming)) => {
                use futures::StreamExt as _;
                let mut stream = match streaming.stream_complete(&req.prompt, &req.params).await {
                    Ok(s) => s,
                    Err(e) => {
                        channel.send(&InferFrame::Error(e.to_string())).await?;
                        continue;
                    }
                };
                let mut text = String::new();
                let mut failed = false;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(tok) => {
                            text.push_str(&tok.text);
                            channel.send(&InferFrame::Token(tok)).await?;
                        }
                        Err(e) => {
                            channel.send(&InferFrame::Error(e.to_string())).await?;
                            failed = true;
                            break;
                        }
                    }
                }
                if !failed {
                    let tool_call = crate::toolcall::parse_tool_call(&text);
                    channel
                        .send(&InferFrame::Done(Completion { text, tool_call }))
                        .await?;
                }
            }
            _ => {
                let frame = match inference.complete(&req.prompt, &req.params).await {
                    Ok(c) => InferFrame::Done(c),
                    Err(e) => InferFrame::Error(e.to_string()),
                };
                channel.send(&frame).await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DummyEngine;
    use futures::StreamExt;

    /// 결정론적 토큰 스트림을 내는 테스트 엔진.
    struct WordEngine;

    #[async_trait]
    impl InferenceEngine for WordEngine {
        async fn complete(&self, prompt: &str, _p: &SamplingParams) -> Result<Completion> {
            Ok(Completion {
                text: prompt.to_uppercase(),
                tool_call: None,
            })
        }
    }

    #[async_trait]
    impl StreamingEngine for WordEngine {
        async fn stream_complete(&self, prompt: &str, _p: &SamplingParams) -> Result<TokenStream> {
            let words: Vec<String> = prompt.split(' ').map(|w| format!("{w} ")).collect();
            let n = words.len();
            let s = stream::iter(words.into_iter().enumerate().map(move |(i, w)| {
                Ok(Token {
                    id: Some(i as u32),
                    text: w,
                    logprob: None,
                    finish_reason: (i + 1 == n).then_some(FinishReason::Eos),
                })
            }));
            Ok(Box::pin(s))
        }
    }

    #[tokio::test]
    async fn non_streaming_roundtrip_over_channel() {
        let (a, b) = lumen_channel::inproc::pair();
        let peer = tokio::spawn(async move { run_peer(b, &DummyEngine::new()).await });
        let engine = ChannelEngine::new(a);
        let c = engine
            .complete("echo over channel", &SamplingParams::default())
            .await
            .unwrap();
        assert_eq!(c.tool_call.unwrap().id.as_str(), "echo");
        drop(engine);
        peer.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn streaming_roundtrip_over_channel_parses_tool_call() {
        let (a, b) = lumen_channel::inproc::pair();
        let engines = Engines::from_dual(Arc::new(WordEngine));
        let peer = tokio::spawn(async move { run_peer_with(b, &engines).await });
        let engine = ChannelEngine::new(a);

        let mut s = engine
            .stream_complete(
                "{\"tool\":\"echo\",\"args\":{}}",
                &SamplingParams::default(),
            )
            .await
            .unwrap();
        let mut toks = Vec::new();
        while let Some(t) = s.next().await {
            toks.push(t.unwrap());
        }
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].finish_reason, Some(FinishReason::Eos));

        // 스트림 종료 후 채널이 동기화되어 다음 요청이 정상 동작해야 합니다.
        let c = engine
            .complete("hello world", &SamplingParams::default())
            .await
            .unwrap();
        assert_eq!(c.text, "HELLO WORLD");
        drop(engine);
        peer.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn dropped_stream_resynchronizes_channel() {
        let (a, b) = lumen_channel::inproc::pair();
        let engines = Engines::from_dual(Arc::new(WordEngine));
        let peer = tokio::spawn(async move { run_peer_with(b, &engines).await });
        let engine = ChannelEngine::new(a);

        {
            let mut s = engine
                .stream_complete("one two three four", &SamplingParams::default())
                .await
                .unwrap();
            let first = s.next().await.unwrap().unwrap();
            assert_eq!(first.text, "one ");
            // 나머지 3 토큰 + Done 을 남기고 drop.
        }
        let c = engine
            .complete("after drop", &SamplingParams::default())
            .await
            .unwrap();
        assert_eq!(c.text, "AFTER DROP");
        drop(engine);
        peer.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn stream_request_to_non_streaming_peer_falls_back() {
        let (a, b) = lumen_channel::inproc::pair();
        let peer = tokio::spawn(async move { run_peer(b, &DummyEngine::new()).await });
        let engine = ChannelEngine::new(a);
        let mut s = engine
            .stream_complete("plain text", &SamplingParams::default())
            .await
            .unwrap();
        let t = s.next().await.unwrap().unwrap();
        assert_eq!(t.finish_reason, Some(FinishReason::Eos));
        assert!(s.next().await.is_none());
        drop(engine);
        peer.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn version_mismatch_rejected() {
        let (mut a, b) = lumen_channel::inproc::pair();
        let peer = tokio::spawn(async move { run_peer(b, &DummyEngine::new()).await });
        a.send(&InferRequest {
            version: 1,
            prompt: "x".into(),
            params: SamplingParams::default(),
            stream: false,
        })
        .await
        .unwrap();
        match a.recv::<InferFrame>().await.unwrap() {
            InferFrame::Error(e) => assert!(e.contains("wire version")),
            other => panic!("unexpected {other:?}"),
        }
        drop(a);
        peer.await.unwrap().unwrap();
    }
}
