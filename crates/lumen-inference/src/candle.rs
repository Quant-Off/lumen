//! candle-onnx 기반 추론 엔진 - `candle` feature 가 활성화된 경우만 컴파일.
//!
//! ## 범위 (v0.3)
//!
//! - 작은 ONNX 분류 모델을 [`std::path::Path`] 로 로드.
//! - 프롬프트를 BLAKE3 해시 -> 고정 길이 `f32` 벡터로 결정론적 인코딩.
//! - 모델 forward 후 출력 벡터의 argmax 를 도구 ID 매핑으로 변환.
//! - 모든 추론 경로는 CPU 만 (CUDA/Metal feature 미활성).
//!
//! ## 결정성
//!
//! candle 의 `f32` 연산은 미세하게 비결정적일 수 있으므로 **이 백엔드의
//! 출력은 ZK witness 에 직접 묶어서는 안 됩니다.** 진정한 결정성 추론은
//! `lumen-fixed` 위에 작성된 자체 quantize 그래프 (v0.5 예정) 에서 다룹니다.
//! 현재는 *Tool routing decision* 에 ONNX 출력을 *입력으로* 사용하되, ZK
//! 증명은 *호스트가 결정한 정수 라우팅 인덱스* 만을 바인드합니다 - float 의
//! 전파를 차단하기 위해서.

use std::path::PathBuf;

use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use lumen_core::{Error, Result, ToolId};

use crate::{Completion, InferenceEngine, SamplingParams, ToolCall};

/// candle-onnx 백엔드. ONNX 모델 한 개를 로드해 inference 에 사용합니다.
pub struct CandleEngine {
    /// ONNX 모델 객체 (candle 의 `ModelProto`).
    model: candle_onnx::onnx::ModelProto,
    /// ONNX 입력 텐서 이름 (모델의 첫 입력).
    input_name: String,
    /// ONNX 입력 길이 (1차원 f32 벡터로 가정).
    input_len: usize,
    /// argmax 인덱스를 ToolId 로 매핑하는 테이블.
    tool_table: Vec<ToolId>,
    /// 진단용 모델 경로 (audit 출력).
    model_path: PathBuf,
}

impl CandleEngine {
    /// 모델 파일 (`.onnx`) 을 로드하고 `tool_table` 을 핀 합니다.
    pub fn load(path: impl Into<PathBuf>, tool_table: Vec<ToolId>) -> Result<Self> {
        let model_path = path.into();
        let model = candle_onnx::read_file(&model_path)
            .map_err(|e| Error::Inference(format!("candle-onnx read: {e}")))?;
        let graph = model
            .graph
            .as_ref()
            .ok_or_else(|| Error::Inference("candle: ONNX graph 없음".into()))?;
        let input_info = graph
            .input
            .first()
            .ok_or_else(|| Error::Inference("candle: ONNX 입력 없음".into()))?;
        let input_name = input_info.name.clone();
        let input_len = input_tensor_length(input_info)?;
        Ok(Self {
            model,
            input_name,
            input_len,
            tool_table,
            model_path,
        })
    }

    /// 프롬프트를 결정론적 `f32` 벡터로 인코딩.
    ///
    /// BLAKE3 다이제스트의 32 바이트를 8 개 `f32` 로 분할한 다음, 모델 입력
    /// 길이에 맞게 반복/잘라냅니다. 진짜 토크나이저는 v0.4 에서.
    fn encode_prompt(&self, prompt: &str) -> Vec<f32> {
        let digest = lumen_core::Blake3Hash::of(prompt.as_bytes());
        let bytes = digest.as_bytes();
        let mut out = Vec::with_capacity(self.input_len);
        for i in 0..self.input_len {
            let byte = bytes[i % bytes.len()];
            // 0..255 -> -1.0..1.0 (대략).
            let v = (byte as f32 / 127.5) - 1.0;
            out.push(v);
        }
        out
    }

    fn run_inference(&self, prompt: &str) -> Result<Vec<f32>> {
        let input_data = self.encode_prompt(prompt);
        let input = Tensor::from_vec(input_data, (1, self.input_len), &Device::Cpu)
            .map_err(|e| Error::Inference(format!("candle tensor: {e}")))?
            .to_dtype(DType::F32)
            .map_err(|e| Error::Inference(format!("candle dtype: {e}")))?;
        let mut inputs = std::collections::HashMap::new();
        inputs.insert(self.input_name.clone(), input);
        let outputs = candle_onnx::simple_eval(&self.model, inputs)
            .map_err(|e| Error::Inference(format!("candle eval: {e}")))?;
        let (_name, tensor) = outputs
            .into_iter()
            .next()
            .ok_or_else(|| Error::Inference("candle: ONNX 출력 없음".into()))?;
        tensor
            .flatten_all()
            .and_then(|t| t.to_vec1::<f32>())
            .map_err(|e| Error::Inference(format!("candle output: {e}")))
    }

    /// 출력 벡터의 argmax 인덱스.
    fn argmax(scores: &[f32]) -> Option<usize> {
        let (idx, _) = scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
        Some(idx)
    }

    /// 모델 경로 (디버그/audit 용).
    pub fn model_path(&self) -> &std::path::Path {
        &self.model_path
    }
}

#[async_trait]
impl InferenceEngine for CandleEngine {
    async fn complete(&self, prompt: &str, _params: &SamplingParams) -> Result<Completion> {
        let scores = self.run_inference(prompt)?;
        let chosen = Self::argmax(&scores).and_then(|i| self.tool_table.get(i).cloned());

        let tool_call = chosen.map(|tool_id| ToolCall {
            id: tool_id,
            args_json: serde_json::json!({ "prompt": prompt }).to_string(),
        });

        Ok(Completion {
            text: format!("[candle] scores={scores:?}"),
            tool_call,
        })
    }
}

fn input_tensor_length(info: &candle_onnx::onnx::ValueInfoProto) -> Result<usize> {
    let ty = info
        .r#type
        .as_ref()
        .ok_or_else(|| Error::Inference("candle: input type 없음".into()))?;
    let value = ty
        .value
        .as_ref()
        .ok_or_else(|| Error::Inference("candle: input value 없음".into()))?;
    use candle_onnx::onnx::type_proto::Value as TPV;
    let TPV::TensorType(tensor_type) = value else {
        return Err(Error::Inference("candle: 비텐서 입력 미지원".into()));
    };
    let shape = tensor_type
        .shape
        .as_ref()
        .ok_or_else(|| Error::Inference("candle: shape 없음".into()))?;
    // 마지막 차원을 입력 길이로 채택. 첫 차원이 batch size 라고 가정.
    let dims = &shape.dim;
    let last = dims
        .last()
        .ok_or_else(|| Error::Inference("candle: 빈 shape".into()))?;
    use candle_onnx::onnx::tensor_shape_proto::dimension::Value as DV;
    match &last.value {
        Some(DV::DimValue(v)) => Ok(*v as usize),
        _ => Err(Error::Inference(
            "candle: 동적 shape 미지원 (마지막 dim 이 정적이어야 함)".into(),
        )),
    }
}
