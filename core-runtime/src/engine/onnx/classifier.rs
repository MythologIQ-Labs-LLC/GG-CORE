//! ONNX-based text classification model.
//!
//! Wraps the Candle ONNX runtime for classification tasks like sentiment
//! analysis. Mirrors [`super::embedder::OnnxEmbedder`]: load a `ModelProto`,
//! run `candle_onnx::simple_eval`, then convert the logits to a
//! [`ClassificationResult`] via a pure, testable helper.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::{
    ClassificationResult, InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
    InferenceOutput,
};

/// ONNX classification model using Candle.
pub struct OnnxClassifier {
    model_id: String,
    // Used to label logits under the `onnx` feature; unused in the stub build.
    #[cfg_attr(not(feature = "onnx"), allow(dead_code))]
    labels: Vec<String>,
    memory_bytes: AtomicUsize,
    #[cfg(feature = "onnx")]
    model: Option<candle_onnx::onnx::ModelProto>,
    #[cfg(feature = "onnx")]
    tokenizer: super::tokenizer::OnnxTokenizer,
}

impl OnnxClassifier {
    /// Create a new classifier with the given model ID and labels (no model
    /// loaded yet — inference fails loud until a model is attached).
    pub fn new(model_id: String, labels: Vec<String>) -> Self {
        Self {
            model_id,
            labels,
            memory_bytes: AtomicUsize::new(0),
            #[cfg(feature = "onnx")]
            model: None,
            #[cfg(feature = "onnx")]
            tokenizer: super::tokenizer::OnnxTokenizer::HashFallback,
        }
    }

    /// Create a classifier with a loaded ONNX model, its label set, and tokenizer.
    #[cfg(feature = "onnx")]
    pub(super) fn with_model(
        model_id: String,
        labels: Vec<String>,
        model: candle_onnx::onnx::ModelProto,
        tokenizer: super::tokenizer::OnnxTokenizer,
    ) -> Self {
        Self {
            model_id,
            labels,
            memory_bytes: AtomicUsize::new(0),
            model: Some(model),
            tokenizer,
        }
    }

    /// Run classification on a single text input.
    fn classify_text(&self, text: &str) -> Result<ClassificationResult, InferenceError> {
        #[cfg(feature = "onnx")]
        {
            self.classify_text_onnx(text)
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = text;
            Err(InferenceError::ModelError(format!(
                "ONNX model '{}' not loaded - enable 'onnx' feature and load model",
                self.model_id
            )))
        }
    }

    #[cfg(feature = "onnx")]
    fn classify_text_onnx(&self, text: &str) -> Result<ClassificationResult, InferenceError> {
        let model = self.model.as_ref().ok_or_else(|| {
            InferenceError::ModelError(format!("model '{}' not loaded", self.model_id))
        })?;

        let device = candle_core::Device::Cpu;
        let tokens = self.tokenizer.encode(text);
        let inputs = super::embedder::build_transformer_inputs(&tokens, &device)?;

        let outputs = candle_onnx::simple_eval(model, inputs)
            .map_err(|e| InferenceError::ModelError(format!("eval: {e}")))?;

        // Deterministic output selection: prefer the named `logits` output;
        // otherwise accept a single-output model. Fail loud on ambiguity rather
        // than picking a nondeterministic HashMap entry.
        let logits = outputs
            .get("logits")
            .or_else(|| (outputs.len() == 1).then(|| outputs.values().next().unwrap()))
            .ok_or_else(|| {
                InferenceError::ModelError(
                    "ambiguous classifier outputs: expected a `logits` output or exactly one output"
                        .into(),
                )
            })?;

        logits_to_classification(logits, &self.labels)
    }
}

/// Convert a logits tensor (`[num_labels]` or `[1, num_labels]`) plus a label
/// set into a [`ClassificationResult`]. Pure — no model required — so it is
/// unit-testable with a synthetic tensor.
#[cfg(feature = "onnx")]
fn logits_to_classification(
    logits: &candle_core::Tensor,
    labels: &[String],
) -> Result<ClassificationResult, InferenceError> {
    let flat = logits
        .flatten_all()
        .map_err(|e| InferenceError::ModelError(format!("flatten: {e}")))?;
    let raw: Vec<f32> = flat
        .to_vec1()
        .map_err(|e| InferenceError::ModelError(format!("logits vec: {e}")))?;

    if raw.len() != labels.len() {
        return Err(InferenceError::ModelError(format!(
            "classifier produced {} logits but {} labels were provided",
            raw.len(),
            labels.len()
        )));
    }

    let probs = softmax(&raw);
    let mut all_labels: Vec<(String, f32)> =
        labels.iter().cloned().zip(probs.iter().copied()).collect();
    all_labels.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let (label, confidence) = all_labels
        .first()
        .cloned()
        .ok_or_else(|| InferenceError::ModelError("empty label set".into()))?;
    Ok(ClassificationResult {
        label,
        confidence,
        all_labels,
    })
}

/// Numerically-stable softmax over a logit slice.
#[cfg(feature = "onnx")]
fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = logits.iter().map(|&x| (x - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    if sum == 0.0 {
        return vec![0.0; logits.len()];
    }
    exp.iter().map(|&x| x / sum).collect()
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
            self.model = None;
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg(feature = "onnx")]
#[path = "classifier_tests.rs"]
mod tests;

#[cfg(test)]
#[cfg(not(feature = "onnx"))]
mod stub_tests {
    use super::*;

    #[test]
    fn classify_text_without_model_fails() {
        let clf = OnnxClassifier::new("c".into(), vec!["a".into(), "b".into()]);
        assert!(clf.classify_text("hello").is_err());
    }
}
