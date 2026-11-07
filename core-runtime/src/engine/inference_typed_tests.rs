//! Typed inference engine tests. Mock outputs are intentionally not proof
//! that real ONNX weights or the authenticated IPC path are qualified.

use super::*;
use crate::engine::{ClassificationResult, EmbeddingResult, InferenceConfig, Model};
use crate::models::ModelHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct TypedMock {
    caps: Vec<InferenceCapability>,
    response: InferenceOutput,
    memory: usize,
    cancel_during_call: Option<Arc<AtomicBool>>,
}

#[async_trait::async_trait]
impl Model for TypedMock {
    fn model_id(&self) -> &str {
        "typed-mock"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &self.caps
    }
    fn memory_usage(&self) -> usize {
        self.memory
    }

    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, crate::engine::InferenceError> {
        if let Some(flag) = &self.cancel_during_call {
            flag.store(true, Ordering::Release);
        }
        Ok(self.response.clone())
    }

    async fn unload(&mut self) -> Result<(), crate::engine::InferenceError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn embedding() -> InferenceOutput {
    InferenceOutput::Embedding(EmbeddingResult {
        vector: vec![0.1, -0.3, 0.4],
        dimensions: 3,
    })
}

fn classification() -> InferenceOutput {
    InferenceOutput::Classification(ClassificationResult {
        label: "positive".into(),
        confidence: 0.8,
        all_labels: vec![("positive".into(), 0.8), ("negative".into(), 0.2)],
    })
}

async fn engine_with(
    capability: InferenceCapability,
    result: InferenceOutput,
    memory: usize,
    cancel_during_call: Option<Arc<AtomicBool>>,
) -> InferenceEngine {
    let engine = InferenceEngine::new(4096);
    engine
        .register_model(
            "typed-mock".into(),
            ModelHandle::new(77),
            Arc::new(TypedMock {
                caps: vec![capability],
                response: result,
                memory,
                cancel_during_call,
            }),
        )
        .await;
    engine
}

async fn execute(
    engine: &InferenceEngine,
    input: InferenceInput,
    capability: InferenceCapability,
    control: &StopControl,
    max_memory_bytes: Option<usize>,
) -> Result<InferenceOutput, InferenceError> {
    engine
        .run_typed_controlled(
            "typed-mock",
            &input,
            capability,
            &InferenceParams::default(),
            control,
            max_memory_bytes,
        )
        .await
}

#[tokio::test]
async fn embedding_text_preserves_typed_vector() {
    let engine = engine_with(InferenceCapability::Embedding, embedding(), 64, None).await;
    let out = execute(
        &engine,
        InferenceInput::Text("Test text".into()),
        InferenceCapability::Embedding,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap();
    let InferenceOutput::Embedding(value) = out else {
        panic!("expected embedding")
    };
    assert_eq!(value.dimensions, 3);
    assert_eq!(value.vector.len(), 3);
}

#[tokio::test]
async fn classification_text_preserves_typed_labels() {
    let engine = engine_with(
        InferenceCapability::TextClassification,
        classification(),
        64,
        None,
    )
    .await;
    let out = execute(
        &engine,
        InferenceInput::Text("A happy sentence".into()),
        InferenceCapability::TextClassification,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap();
    let InferenceOutput::Classification(value) = out else {
        panic!("expected classification")
    };
    assert_eq!(value.label, "positive");
    assert_eq!(value.confidence, 0.8);
}

#[tokio::test]
async fn embedding_batch_preserves_exact_count() {
    let items = vec![
        EmbeddingResult {
            vector: vec![0.1, 0.2],
            dimensions: 2,
        },
        EmbeddingResult {
            vector: vec![0.3, 0.4],
            dimensions: 2,
        },
    ];
    let engine = engine_with(
        InferenceCapability::Embedding,
        InferenceOutput::EmbeddingBatch(items),
        64,
        None,
    )
    .await;
    let out = execute(
        &engine,
        InferenceInput::TextBatch(vec!["one".into(), "two".into()]),
        InferenceCapability::Embedding,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(out, InferenceOutput::EmbeddingBatch(batch) if batch.len() == 2));
}

#[tokio::test]
async fn batch_count_mismatch_fails_closed() {
    let engine = engine_with(
        InferenceCapability::Embedding,
        InferenceOutput::EmbeddingBatch(vec![EmbeddingResult {
            vector: vec![1.0],
            dimensions: 1,
        }]),
        64,
        None,
    )
    .await;
    let err = execute(
        &engine,
        InferenceInput::TextBatch(vec!["one".into(), "two".into()]),
        InferenceCapability::Embedding,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::ExecutionFailed(_)));
}

#[tokio::test]
async fn non_finite_embedding_fails_closed() {
    let bad = InferenceOutput::Embedding(EmbeddingResult {
        vector: vec![f32::NAN, 0.1],
        dimensions: 2,
    });
    let engine = engine_with(InferenceCapability::Embedding, bad, 64, None).await;
    let err = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::Embedding,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::ExecutionFailed(_)));
}

#[tokio::test]
async fn output_variant_cannot_impersonate_requested_task() {
    let engine = engine_with(InferenceCapability::Embedding, classification(), 64, None).await;
    let err = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::Embedding,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::ExecutionFailed(_)));
}

#[tokio::test]
async fn model_capability_is_checked_before_execution() {
    let engine = engine_with(InferenceCapability::Embedding, embedding(), 64, None).await;
    let err = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::TextClassification,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::InvalidParams(_)));
}

#[tokio::test]
async fn unsupported_classification_batch_refused() {
    let engine = engine_with(
        InferenceCapability::TextClassification,
        classification(),
        64,
        None,
    )
    .await;
    let err = execute(
        &engine,
        InferenceInput::TextBatch(vec!["one".into(), "two".into()]),
        InferenceCapability::TextClassification,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::InvalidParams(_)));
}

#[tokio::test]
async fn working_budget_is_not_compared_to_resident_weights() {
    // Regression (2026-11-07): weights are reserved once at load; a model
    // larger than the per-request working budget must still serve.
    let engine = engine_with(InferenceCapability::Embedding, embedding(), 2048, None).await;
    let out = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::Embedding,
        &StopControl::default(),
        Some(1024),
    )
    .await;
    assert!(matches!(out, Ok(InferenceOutput::Embedding(_))), "{out:?}");
}

#[tokio::test]
async fn pre_cancelled_typed_call_fails_before_model_execution() {
    let engine = engine_with(InferenceCapability::Embedding, embedding(), 64, None).await;
    let cancelled = Arc::new(AtomicBool::new(true));
    let ctrl = StopControl::from_timeout(Some(cancelled), None);
    let err = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::Embedding,
        &ctrl,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::Cancelled));
}

#[tokio::test]
async fn cancellation_during_backend_call_never_returns_partial_vector() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let engine = engine_with(
        InferenceCapability::Embedding,
        embedding(),
        64,
        Some(Arc::clone(&cancelled)),
    )
    .await;
    let ctrl = StopControl::from_timeout(Some(cancelled), None);
    let err = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::Embedding,
        &ctrl,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::Cancelled));
}

#[tokio::test]
async fn classification_nan_confidence_rejected() {
    let bad = InferenceOutput::Classification(ClassificationResult {
        label: "positive".into(),
        confidence: f32::NAN,
        all_labels: vec![("positive".into(), f32::NAN)],
    });
    let engine = engine_with(InferenceCapability::TextClassification, bad, 64, None).await;
    let err = execute(
        &engine,
        InferenceInput::Text("hi".into()),
        InferenceCapability::TextClassification,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::ExecutionFailed(_)));
}

#[tokio::test]
async fn invalid_empty_text_refused() {
    let engine = engine_with(InferenceCapability::Embedding, embedding(), 64, None).await;
    let err = execute(
        &engine,
        InferenceInput::Text("".into()),
        InferenceCapability::Embedding,
        &StopControl::default(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, InferenceError::InvalidParams(_)));
}

#[tokio::test]
async fn no_typed_text_generation_or_ner_bypass_is_available() {
    let engine = engine_with(InferenceCapability::Embedding, embedding(), 64, None).await;
    for cap in [
        InferenceCapability::TextGeneration,
        InferenceCapability::NamedEntityRecognition,
    ] {
        let err = execute(
            &engine,
            InferenceInput::Text("hi".into()),
            cap,
            &StopControl::default(),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, InferenceError::InvalidParams(_)));
    }
}
