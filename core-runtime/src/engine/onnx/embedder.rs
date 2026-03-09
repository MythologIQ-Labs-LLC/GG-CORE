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
    _model: Option<()>, // Placeholder for candle model
}

impl OnnxEmbedder {
    /// Create a new embedder with the given model ID and embedding dimension.
    pub fn new(model_id: String, embedding_dim: usize) -> Self {
        Self {
            model_id,
            embedding_dim,
            memory_bytes: AtomicUsize::new(0),
            #[cfg(feature = "onnx")]
            _model: None,
        }
    }

    /// Generate embedding for a single text input.
    fn embed_text(&self, _text: &str) -> Result<EmbeddingResult, InferenceError> {
        // ONNX model not loaded - fail rather than return mock data
        // Real implementation requires candle-onnx with loaded model
        Err(InferenceError::ModelError(format!(
            "ONNX model '{}' not loaded - enable 'onnx' feature and load model",
            self.model_id
        )))
    }
}

#[async_trait::async_trait]
impl super::OnnxModel for OnnxEmbedder {
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
                // Embed first item for now (batch support would return multiple)
                let text = batch.first().ok_or_else(|| {
                    InferenceError::InputValidation("batch cannot be empty".into())
                })?;
                let result = self.embed_text(text)?;
                Ok(InferenceOutput::Embedding(result))
            }
            InferenceInput::ChatMessages(_) => Err(InferenceError::CapabilityNotSupported(
                "chat messages not supported for embedding".into(),
            )),
        }
    }

    async fn unload(&mut self) -> Result<(), InferenceError> {
        self.memory_bytes.store(0, Ordering::SeqCst);
        #[cfg(feature = "onnx")]
        {
            self._model = None;
        }
        Ok(())
    }
}
