//! ONNX-based text classification model.
//!
//! Wraps Candle ONNX runtime for classification tasks like sentiment analysis.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::{
    ClassificationResult, InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
    InferenceOutput,
};

/// ONNX classification model using Candle.
pub struct OnnxClassifier {
    model_id: String,
    #[allow(dead_code)]
    labels: Vec<String>,
    memory_bytes: AtomicUsize,
    #[cfg(feature = "onnx")]
    _model: Option<()>, // Placeholder for candle model
}

impl OnnxClassifier {
    /// Create a new classifier with the given model ID and labels.
    pub fn new(model_id: String, labels: Vec<String>) -> Self {
        Self {
            model_id,
            labels,
            memory_bytes: AtomicUsize::new(0),
            #[cfg(feature = "onnx")]
            _model: None,
        }
    }

    /// Run classification on a single text input.
    fn classify_text(&self, _text: &str) -> Result<ClassificationResult, InferenceError> {
        // ONNX model not loaded - fail rather than return mock data
        // Real implementation requires candle-onnx with loaded model
        Err(InferenceError::ModelError(format!(
            "ONNX model '{}' not loaded - enable 'onnx' feature and load model",
            self.model_id
        )))
    }
}

#[async_trait::async_trait]
impl super::OnnxModel for OnnxClassifier {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextClassification]
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
                let result = self.classify_text(text)?;
                Ok(InferenceOutput::Classification(result))
            }
            InferenceInput::TextBatch(batch) => {
                // Classify first item for now (batch support would aggregate)
                let text = batch.first().ok_or_else(|| {
                    InferenceError::InputValidation("batch cannot be empty".into())
                })?;
                let result = self.classify_text(text)?;
                Ok(InferenceOutput::Classification(result))
            }
            InferenceInput::ChatMessages(_) => Err(InferenceError::CapabilityNotSupported(
                "chat messages not supported for classification".into(),
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
