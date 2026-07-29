//! ONNX-based embedding model.
//!
//! Wraps Candle ONNX runtime for generating text embeddings.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::{
    EmbeddingResult, InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
    InferenceOutput,
};

/// ONNX embedding model using Candle.
pub struct OnnxEmbedder {
    model_id: String,
    #[allow(dead_code)]
    embedding_dim: usize,
    memory_bytes: AtomicUsize,
    #[cfg(feature = "onnx")]
    model: Option<candle_onnx::onnx::ModelProto>,
    #[cfg(feature = "onnx")]
    tokenizer: super::tokenizer::OnnxTokenizer,
}

impl OnnxEmbedder {
    /// Create a new embedder stub (no model loaded).
    pub fn new(model_id: String, embedding_dim: usize) -> Self {
        Self {
            model_id,
            embedding_dim,
            memory_bytes: AtomicUsize::new(0),
            #[cfg(feature = "onnx")]
            model: None,
            #[cfg(feature = "onnx")]
            tokenizer: super::tokenizer::OnnxTokenizer::HashFallback,
        }
    }

    /// Create an embedder with a loaded Candle ONNX model and its tokenizer.
    #[cfg(feature = "onnx")]
    pub(super) fn with_model(
        model_id: String,
        embedding_dim: usize,
        model: candle_onnx::onnx::ModelProto,
        tokenizer: super::tokenizer::OnnxTokenizer,
    ) -> Self {
        Self {
            model_id,
            embedding_dim,
            memory_bytes: AtomicUsize::new(0),
            model: Some(model),
            tokenizer,
        }
    }

    /// Generate embedding for a single text input.
    fn embed_text(&self, text: &str) -> Result<EmbeddingResult, InferenceError> {
        #[cfg(feature = "onnx")]
        {
            self.embed_text_onnx(text)
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = text;
            Err(InferenceError::ModelError(
                "onnx feature not enabled".into(),
            ))
        }
    }

    /// Run ONNX inference to produce an embedding vector.
    #[cfg(feature = "onnx")]
    fn embed_text_onnx(&self, text: &str) -> Result<EmbeddingResult, InferenceError> {
        let model = self.model.as_ref().ok_or_else(|| {
            InferenceError::ModelError(format!("model '{}' not loaded", self.model_id))
        })?;

        let device = candle_core::Device::Cpu;
        let tokens = self.tokenizer.encode(text);
        let inputs = build_transformer_inputs(&tokens, &device)?;

        let outputs = candle_onnx::simple_eval(model, inputs)
            .map_err(|e| InferenceError::ModelError(format!("eval: {e}")))?;

        let tensor = outputs
            .values()
            .next()
            .ok_or_else(|| InferenceError::ModelError("no output tensor".into()))?;

        let pooled =
            mean_pool(tensor).map_err(|e| InferenceError::ModelError(format!("pool: {e}")))?;

        let vector: Vec<f32> = pooled
            .to_vec1()
            .map_err(|e| InferenceError::ModelError(format!("vec: {e}")))?;

        let dimensions = vector.len();
        Ok(EmbeddingResult { vector, dimensions })
    }
}

/// Build input_ids, attention_mask, token_type_ids tensors.
#[cfg(feature = "onnx")]
pub(super) fn build_transformer_inputs(
    tokens: &[i64],
    device: &candle_core::Device,
) -> Result<std::collections::HashMap<String, candle_core::Tensor>, InferenceError> {
    let ids = candle_core::Tensor::new(tokens, device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| InferenceError::ModelError(format!("input: {e}")))?;

    let attn = candle_core::Tensor::ones_like(&ids)
        .map_err(|e| InferenceError::ModelError(format!("attn: {e}")))?;

    let ttype = candle_core::Tensor::zeros_like(&ids)
        .map_err(|e| InferenceError::ModelError(format!("ttype: {e}")))?;

    let mut map = std::collections::HashMap::new();
    map.insert("input_ids".to_string(), ids);
    map.insert("attention_mask".to_string(), attn);
    map.insert("token_type_ids".to_string(), ttype);
    Ok(map)
}

/// Mean-pool across sequence dimension (dim 1) and squeeze batch.
#[cfg(feature = "onnx")]
fn mean_pool(tensor: &candle_core::Tensor) -> candle_core::Result<candle_core::Tensor> {
    // tensor shape: [1, seq_len, hidden_dim] -> [hidden_dim]
    tensor.mean(1)?.squeeze(0)
}

#[async_trait::async_trait]
impl crate::engine::Model for OnnxEmbedder {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::Embedding]
    }

    fn memory_usage(&self) -> usize {
        self.memory_bytes.load(Ordering::SeqCst)
    }

    async fn infer(
        &self,
        input: &InferenceInput,
        _config: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        input.validate()?;
        match input {
            InferenceInput::Text(text) => {
                let result = self.embed_text(text)?;
                Ok(InferenceOutput::Embedding(result))
            }
            InferenceInput::TextBatch(batch) => {
                let text = batch.first().ok_or_else(|| {
                    InferenceError::InputValidation("batch cannot be empty".into())
                })?;
                let result = self.embed_text(text)?;
                Ok(InferenceOutput::Embedding(result))
            }
            InferenceInput::ChatMessages(_) => Err(InferenceError::CapabilityNotSupported(
                "chat not supported for embedding".into(),
            )),
        }
    }

    async fn unload(&mut self) -> Result<(), InferenceError> {
        self.memory_bytes.store(0, Ordering::SeqCst);
        #[cfg(feature = "onnx")]
        {
            self.model = None;
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
#[cfg(feature = "onnx")]
mod tests {
    use super::*;

    fn model_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/models/onnx/all-MiniLM-L6-v2.onnx")
    }

    #[test]
    fn load_and_embed() {
        // The ONNX fixture (~90 MB) is not committed; skip gracefully when
        // absent (e.g. CI) — same convention as the gguf e2e tests.
        let path = model_path();
        if !path.exists() {
            eprintln!(
                "skipping load_and_embed: fixture {} not present",
                path.display()
            );
            return;
        }
        let model = candle_onnx::read_file(&path).expect("load model");
        let embedder = OnnxEmbedder::with_model(
            "test".into(),
            384,
            model,
            super::super::tokenizer::OnnxTokenizer::for_model(&path),
        );
        let result = embedder.embed_text("file.write").expect("embed");
        assert_eq!(result.vector.len(), 384);
        assert!(result.vector.iter().any(|&v| v != 0.0));
    }

    #[test]
    fn missing_model_fails() {
        let embedder = OnnxEmbedder::new("missing".into(), 384);
        assert!(embedder.embed_text("test").is_err());
    }
}
