//! candle-transformers 기반 LLM 추론 엔진 — `candle-llm` feature 가 활성화된 경우만 컴파일.
//!
//! ## 지원 모델
//!
//! GGUF 양자화 가중치 (LLaMA / Mistral 계열) 를 candle-transformers 의
//! `quantized_llama::ModelWeights` 로 로드합니다. Safetensors 포맷은
//! 추후 지원 예정입니다.
//!
//! ## 보안 불변식
//!
//! 생성자 [`CandleLlmEngine::from_gguf`] 는 raw 경로 대신
//! [`VerifiedModelHandle`] 을 받습니다. 검증을 건너뛰면 컴파일 에러가
//! 발생합니다.
//!
//! ## 스트리밍
//!
//! [`StreamingEngine::stream_complete`] 는 별도 `tokio::task` 에서 생성 루프를
//! 실행하고 [`tokio::sync::mpsc`] 채널을 통해 토큰을 전달합니다.
//! 스트림을 drop 하면 채널이 닫히고 task 가 자동 종료됩니다.
//!
//! ## 결정성 주의
//!
//! candle 의 f32 연산은 CPU 아키텍처 간 미세하게 비결정적일 수 있습니다.
//! 텍스트 생성 출력은 ZK witness 에 직접 바인딩하지 마세요. 도구 라우팅
//! *결정* (정수 인덱스) 만 ZK 에 바인딩하며, `CandleEngine` (ONNX) 과
//! 동일한 접근 방식을 따릅니다.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama::ModelWeights;
use futures::stream;
use lumen_core::{Error, Result};
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

use crate::loader::VerifiedModelHandle;
use crate::streaming::{FinishReason, StreamingEngine, Token, TokenStream};
use crate::{Completion, InferenceEngine, SamplingParams};

/// candle-transformers 기반 LLM 엔진.
///
/// 생성은 항상 CPU 에서 실행됩니다 (`Device::Cpu`). GPU/Metal 지원은
/// 추후 feature flag 로 추가 예정입니다.
pub struct CandleLlmEngine {
    model: Arc<Mutex<ModelWeights>>,
    tokenizer: Arc<Tokenizer>,
    eos_token_id: Option<u32>,
}

impl CandleLlmEngine {
    /// GGUF 양자화 모델을 검증된 핸들로부터 로드합니다.
    ///
    /// `tokenizer_path` 는 HuggingFace `tokenizer.json` 형식이어야 합니다.
    ///
    /// # 에러
    ///
    /// - 파일 열기 실패 → [`Error::Inference`]
    /// - GGUF 파싱 실패 → [`Error::Inference`]
    /// - 토크나이저 로드 실패 → [`Error::Inference`]
    pub fn from_gguf(
        handle: &VerifiedModelHandle,
        tokenizer_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let device = Device::Cpu;

        tracing::info!(
            target: "lumen.inference.candle_llm",
            path = ?handle.path(),
            model = %handle.model_info.name,
            "GGUF 모델 로딩 중"
        );

        // GGUF 파싱
        let mut file = std::fs::File::open(handle.path())
            .map_err(|e| Error::Inference(format!("GGUF 파일 열기 실패: {e}")))?;
        let gguf_content = candle_core::quantized::gguf_file::Content::read(&mut file)
            .map_err(|e| Error::Inference(format!("GGUF 파싱 실패: {e}")))?;
        let model = ModelWeights::from_gguf(gguf_content, &mut file, &device)
            .map_err(|e| Error::Inference(format!("모델 가중치 로드 실패: {e}")))?;

        // 토크나이저 로드
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| Error::Inference(format!("토크나이저 로드 실패: {e}")))?;

        // EOS 토큰 탐색 (LLaMA: </s>, GPT-계열: <|endoftext|>)
        let eos_token_id = tokenizer
            .token_to_id("</s>")
            .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
            .or_else(|| tokenizer.token_to_id("<eos>"));

        tracing::info!(
            target: "lumen.inference.candle_llm",
            eos_token_id,
            "모델 로드 완료"
        );

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            tokenizer: Arc::new(tokenizer),
            eos_token_id,
        })
    }

    /// 프롬프트를 토큰 ID 벡터로 인코딩합니다.
    fn encode_prompt(&self, prompt: &str) -> Result<Vec<u32>> {
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| Error::Inference(format!("인코딩 실패: {e}")))?;
        Ok(encoding.get_ids().to_vec())
    }
}

#[async_trait]
impl InferenceEngine for CandleLlmEngine {
    /// 스트림을 내부적으로 소진해 전체 completion 텍스트로 조립합니다.
    async fn complete(&self, prompt: &str, params: &SamplingParams) -> Result<Completion> {
        use futures::StreamExt as _;
        let mut token_stream = self.stream_complete(prompt, params).await?;
        let mut text = String::new();
        while let Some(tok) = token_stream.next().await {
            text.push_str(&tok?.text);
        }
        Ok(Completion {
            text,
            tool_call: None,
        })
    }
}

#[async_trait]
impl StreamingEngine for CandleLlmEngine {
    async fn stream_complete(
        &self,
        prompt: &str,
        params: &SamplingParams,
    ) -> Result<TokenStream> {
        let prompt_tokens = self.encode_prompt(prompt)?;
        if prompt_tokens.is_empty() {
            return Err(Error::Inference("프롬프트 토큰화 결과가 비어있습니다".into()));
        }

        let max_tokens = params.max_tokens as usize;
        let seed = params.seed.unwrap_or(0);
        let temperature = params.temperature;
        let top_p = params.top_p;
        let eos_token_id = self.eos_token_id;
        let model = Arc::clone(&self.model);
        let tokenizer = Arc::clone(&self.tokenizer);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Token>>(128);

        tokio::spawn(async move {
            if let Err(e) = run_generation(
                model,
                tokenizer,
                prompt_tokens,
                max_tokens,
                temperature,
                top_p,
                eos_token_id,
                seed,
                tx.clone(),
            )
            .await
            {
                // 채널이 이미 닫혔으면(호출자가 drop) 에러 전송은 무시합니다.
                let _ = tx.send(Err(e)).await;
            }
        });

        // mpsc Receiver → futures::Stream 변환
        let s = stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });

        Ok(Box::pin(s))
    }
}

/// 생성 루프 (별도 tokio task 에서 실행).
///
/// 모델 잠금을 보유한 채 토큰을 순차적으로 생성하고 `tx` 로 전달합니다.
/// 채널 수신자가 drop 되면 (`tx.send` 실패) 즉시 종료합니다.
async fn run_generation(
    model: Arc<Mutex<ModelWeights>>,
    tokenizer: Arc<Tokenizer>,
    prompt_tokens: Vec<u32>,
    max_tokens: usize,
    temperature: f32,
    top_p: f32,
    eos_token_id: Option<u32>,
    seed: u64,
    tx: tokio::sync::mpsc::Sender<Result<Token>>,
) -> Result<()> {
    let device = Device::Cpu;

    // LogitsProcessor: temperature == 0 이면 greedy decoding.
    let temp_opt = if temperature == 0.0 {
        None
    } else {
        Some(temperature as f64)
    };
    let top_p_opt = if top_p > 0.0 && top_p < 1.0 {
        Some(top_p as f64)
    } else {
        None
    };
    let mut logits_processor = LogitsProcessor::new(seed, temp_opt, top_p_opt);

    let prompt_len = prompt_tokens.len();

    // 모델 잠금을 보유한 채 프롬프트 prefill + 첫 토큰 샘플링
    let mut model_guard = model.lock().await;

    let input = Tensor::new(prompt_tokens.as_slice(), &device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| Error::Inference(format!("프롬프트 텐서 생성 실패: {e}")))?;

    // prefill — KV 캐시 위치 0부터 시작
    let logits = model_guard
        .forward(&input, 0)
        .map_err(|e| Error::Inference(format!("prefill forward 실패: {e}")))?;

    // 마지막 위치의 로짓 추출: [1, seq_len, vocab] → [vocab]
    let logits = extract_last_logit(logits, prompt_len)?;

    let mut next_token = logits_processor
        .sample(&logits)
        .map_err(|e| Error::Inference(format!("샘플링 실패: {e}")))?;

    let mut pos = prompt_len;

    for step in 0..max_tokens {
        let decoded = tokenizer
            .decode(&[next_token], true)
            .map_err(|e| Error::Inference(format!("토큰 디코딩 실패: {e}")))?;

        let is_eos = eos_token_id.map_or(false, |eos| next_token == eos);
        let finish_reason = if is_eos {
            Some(FinishReason::Eos)
        } else if step + 1 >= max_tokens {
            Some(FinishReason::MaxTokens)
        } else {
            None
        };

        let token = Token {
            id: next_token,
            text: decoded,
            logprob: None,
            finish_reason,
        };

        // 채널 수신자가 drop 되면 생성을 중단합니다.
        if tx.send(Ok(token)).await.is_err() {
            break;
        }

        if is_eos || finish_reason.is_some() {
            break;
        }

        // 다음 토큰 생성
        let input = Tensor::new(&[next_token], &device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| Error::Inference(format!("단일 토큰 텐서 실패: {e}")))?;

        let logits = model_guard
            .forward(&input, pos)
            .map_err(|e| Error::Inference(format!("autoregressive forward 실패: {e}")))?;

        let logits = extract_last_logit(logits, 1)?;

        next_token = logits_processor
            .sample(&logits)
            .map_err(|e| Error::Inference(format!("샘플링 실패: {e}")))?;

        pos += 1;
    }

    Ok(())
}

/// `[1, n, vocab]` 텐서에서 위치 `n - 1` 의 로짓 벡터 `[vocab]` 를 추출합니다.
fn extract_last_logit(logits: Tensor, n: usize) -> Result<Tensor> {
    // [1, n, vocab] → [n, vocab]
    let logits = logits
        .squeeze(0)
        .map_err(|e| Error::Inference(format!("batch squeeze 실패: {e}")))?;
    // [n, vocab] → [vocab]  (마지막 시퀀스 위치)
    let logits = logits
        .get(n - 1)
        .map_err(|e| Error::Inference(format!("마지막 로짓 추출 실패: {e}")))?;
    // 양자화 텐서를 f32 로 변환 (LogitsProcessor 요구사항)
    logits
        .to_dtype(DType::F32)
        .map_err(|e| Error::Inference(format!("f32 변환 실패: {e}")))
}
