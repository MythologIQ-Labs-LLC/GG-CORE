//! ONNX-based embedding model.
//!
//! Wraps the Candle ONNX runtime for generating text embeddings: tokenize →
//! `simple_eval` → deterministic hidden-state selection → attention-masked
//! mean pooling → L2 normalization. Target model shape is MiniLM-style
//! (`all-MiniLM-L6-v2`, 384 dims) but nothing model-specific is hardcoded.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::{
    EmbeddingResult, InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
    InferenceOutput,
};

/// ONNX embedding model using Candle.
pub struct OnnxEmbedder {
    model_id: String,
    embedding_dim: usize,
    memory_bytes: AtomicUsize,
    #[cfg(feature = "onnx")]
    model: Option<candle_onnx::onnx::ModelProto>,
    #[cfg(feature = "onnx")]
    tokenizer: super::tokenizer::OnnxTokenizer,
}

impl OnnxEmbedder {
    /// Create a new embedder in the not-loaded state — inference fails loud
    /// until a model is attached via [`OnnxEmbedder::load`].
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

    /// Load an embedder from `<model_dir>/<model_id>/model.onnx` with its
    /// sibling `tokenizer.json` (offline, read-only — no network path exists).
    ///
    /// # Errors
    /// Fails loud when `model.onnx` is missing or not a valid ONNX model. A
    /// missing `tokenizer.json` degrades to the logged hash fallback (B-28).
    /// A present invalid tokenizer is refused rather than silently downgraded.
    #[cfg(feature = "onnx")]
    pub fn load(
        model_dir: &Path,
        model_id: &str,
        embedding_dim: usize,
    ) -> Result<Self, InferenceError> {
        let model_path = model_dir.join(model_id).join("model.onnx");
        let model = candle_onnx::read_file(&model_path)
            .map_err(|e| InferenceError::ModelError(format!("load {model_path:?}: {e}")))?;
        let tokenizer = super::tokenizer::OnnxTokenizer::for_model(&model_path)?;
        Ok(Self::with_model(
            model_id.to_string(),
            embedding_dim,
            model,
            tokenizer,
        ))
    }

    /// Stub for non-onnx builds: always fails loud so callers compile either way.
    #[cfg(not(feature = "onnx"))]
    pub fn load(
        _model_dir: &Path,
        _model_id: &str,
        _embedding_dim: usize,
    ) -> Result<Self, InferenceError> {
        Err(InferenceError::ModelError(
            "onnx feature not enabled".into(),
        ))
    }

    /// Create an embedder with a loaded Candle ONNX model and its tokenizer.
    #[cfg(feature = "onnx")]
    pub(super) fn with_model(
        model_id: String,
        embedding_dim: usize,
        model: candle_onnx::onnx::ModelProto,
        tokenizer: super::tokenizer::OnnxTokenizer,
    ) -> Self {
        let estimate = super::tensor_ops::model_memory_estimate(&model);
        Self {
            model_id,
            embedding_dim,
            memory_bytes: AtomicUsize::new(estimate),
            model: Some(model),
            tokenizer,
        }
    }

    /// Expected embedding dimensionality of the wrapped model.
    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
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

    /// Run ONNX inference to produce a unit-length embedding vector.
    #[cfg(feature = "onnx")]
    fn embed_text_onnx(&self, text: &str) -> Result<EmbeddingResult, InferenceError> {
        use super::tensor_ops;

        let model = self.model.as_ref().ok_or_else(|| {
            InferenceError::ModelError(format!("model '{}' not loaded", self.model_id))
        })?;

        let encoded = self.tokenizer.encode(text)?;
        if !encoded.attention_mask.contains(&1) {
            return Err(InferenceError::InputValidation(
                "input tokenized to zero attended tokens".into(),
            ));
        }

        let device = candle_core::Device::Cpu;
        let inputs = tensor_ops::build_transformer_inputs(&encoded, &device)?;
        let attention_mask = inputs["attention_mask"].clone();

        let outputs = candle_onnx::simple_eval(model, inputs)
            .map_err(|e| InferenceError::ModelError(format!("eval: {e}")))?;
        let hidden = tensor_ops::select_hidden_state(&outputs)?;

        let pooled = tensor_ops::masked_mean_pool(hidden, &attention_mask)
            .map_err(|e| InferenceError::ModelError(format!("pool: {e}")))?;
        let raw: Vec<f32> = pooled
            .to_vec1()
            .map_err(|e| InferenceError::ModelError(format!("vec: {e}")))?;

        // The loaded model must honor the expected embedding dimension.
        // A successful but different-width result silently corrupts consumer
        // vector indexes and breaks cross-model compatibility.
        if raw.len() != self.embedding_dim || self.embedding_dim == 0 {
            return Err(InferenceError::ModelError(format!(
                "embedding dimension mismatch: expected {}, model produced {}",
                self.embedding_dim,
                raw.len()
            )));
        }
        let vector = tensor_ops::l2_normalize(raw)?;
        Ok(EmbeddingResult {
            vector,
            dimensions: self.embedding_dim,
        })
    }
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
        config: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        self.infer_cancellable(input, config, None).await
    }

    async fn infer_cancellable(
        &self,
        input: &InferenceInput,
        _config: &InferenceConfig,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, InferenceError> {
        input.validate()?;
        let stop = || {
            if is_cancelled.is_some_and(|check| check()) {
                Err(InferenceError::Cancelled)
            } else {
                Ok(())
            }
        };
        stop()?;
        match input {
            InferenceInput::Text(text) => {
                let result = self.embed_text(text)?;
                stop()?;
                Ok(InferenceOutput::Embedding(result))
            }
            InferenceInput::TextBatch(batch) => {
                let mut results = Vec::with_capacity(batch.len());
                for text in batch {
                    stop()?;
                    let result = self.embed_text(text)?;
                    stop()?;
                    results.push(result);
                }
                Ok(InferenceOutput::EmbeddingBatch(results))
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
#[path = "embedder_tests.rs"]
mod tests;
#[cfg(all(test, feature = "onnx"))]
#[path = "embedder_cancel_tests.rs"]
mod cancel_tests;
