//! Tests for unified model lifecycle.

use std::sync::Arc;

use super::*;
use crate::engine::gguf::GgufModel;
use crate::engine::{
    InferenceCapability, InferenceConfig, InferenceError as EngineError,
    InferenceInput, InferenceOutput, GenerationResult,
};

/// Mock model for testing lifecycle without real GGUF backend.
struct MockModel {
    id: String,
    memory: usize,
}

impl MockModel {
    fn new(id: &str, memory: usize) -> Arc<dyn GgufModel> {
        Arc::new(Self { id: id.to_string(), memory })
    }
}

#[async_trait::async_trait]
impl GgufModel for MockModel {
    fn model_id(&self) -> &str { &self.id }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }

    fn memory_usage(&self) -> usize { self.memory }

    async fn infer(
        &self,
        _input: &InferenceInput,
        _config: &InferenceConfig,
    ) -> Result<InferenceOutput, EngineError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "mock output".into(),
            tokens_generated: 2,
            finish_reason: crate::engine::FinishReason::MaxTokens,
        }))
    }

    async fn unload(&mut self) -> Result<(), EngineError> { Ok(()) }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

fn make_metadata(name: &str, size: u64) -> ModelMetadata {
    ModelMetadata { name: name.to_string(), size_bytes: size }
}

fn make_lifecycle() -> ModelLifecycle {
    let registry = Arc::new(ModelRegistry::new());
    let engine = Arc::new(InferenceEngine::new(4096));
    ModelLifecycle::new(registry, engine)
}

#[tokio::test]
async fn load_registers_in_both_registries() {
    let lc = make_lifecycle();
    let model = MockModel::new("test-model", 1024);
    let meta = make_metadata("test-model", 1024);

    let handle = lc.load("test-model".into(), meta, model).await.unwrap();

    assert!(lc.registry.contains(handle).await);
    assert!(lc.engine.has_model("test-model").await);
    assert!(lc.is_loaded("test-model").await);
}

#[tokio::test]
async fn bidirectional_o1_lookup() {
    let lc = make_lifecycle();
    let model = MockModel::new("fast-model", 2048);
    let meta = make_metadata("fast-model", 2048);

    let handle = lc.load("fast-model".into(), meta, model).await.unwrap();

    let found = lc.get_handle("fast-model").await.unwrap();
    assert_eq!(found.id(), handle.id());

    let found_id = lc.get_model_id(handle.id()).await.unwrap();
    assert_eq!(found_id, "fast-model");
}

#[tokio::test]
async fn unload_removes_from_both_registries() {
    let lc = make_lifecycle();
    let model = MockModel::new("bye-model", 512);
    let meta = make_metadata("bye-model", 512);

    let handle = lc.load("bye-model".into(), meta, model).await.unwrap();
    assert!(lc.is_loaded("bye-model").await);

    lc.unload("bye-model").await.unwrap();

    assert!(!lc.registry.contains(handle).await);
    assert!(!lc.engine.has_model("bye-model").await);
    assert!(!lc.is_loaded("bye-model").await);
    assert!(lc.get_handle("bye-model").await.is_none());
    assert!(lc.get_model_id(handle.id()).await.is_none());
}

#[tokio::test]
async fn partial_state_impossible_duplicate_load_rejected() {
    let lc = make_lifecycle();
    let model1 = MockModel::new("dup", 100);
    let meta1 = make_metadata("dup", 100);

    lc.load("dup".into(), meta1, model1).await.unwrap();

    let model2 = MockModel::new("dup", 200);
    let meta2 = make_metadata("dup", 200);
    let err = lc.load("dup".into(), meta2, model2).await.unwrap_err();
    assert!(matches!(err, LifecycleError::AlreadyLoaded(_)));

    assert_eq!(lc.count().await, 1);
}

#[tokio::test]
async fn unload_nonexistent_returns_error() {
    let lc = make_lifecycle();
    let err = lc.unload("ghost").await.unwrap_err();
    assert!(matches!(err, LifecycleError::NotLoaded(_)));
}

#[tokio::test]
async fn memory_tracked_in_registry() {
    let lc = make_lifecycle();
    let model = MockModel::new("mem-test", 4096);
    let meta = make_metadata("mem-test", 4096);

    lc.load("mem-test".into(), meta, model).await.unwrap();

    assert_eq!(lc.registry.total_memory().await, 4096);
}

#[tokio::test]
async fn inference_works_through_lifecycle() {
    let lc = make_lifecycle();
    let model = MockModel::new("infer-test", 1024);
    let meta = make_metadata("infer-test", 1024);

    lc.load("infer-test".into(), meta, model).await.unwrap();

    let params = crate::engine::InferenceParams::default();
    let result = lc.engine.run("infer-test", "hello", &params).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().output, "mock output");
}

#[tokio::test]
async fn multiple_models_independent() {
    let lc = make_lifecycle();

    let m1 = MockModel::new("a", 100);
    let m2 = MockModel::new("b", 200);

    lc.load("a".into(), make_metadata("a", 100), m1).await.unwrap();
    lc.load("b".into(), make_metadata("b", 200), m2).await.unwrap();

    assert_eq!(lc.count().await, 2);

    lc.unload("a").await.unwrap();
    assert_eq!(lc.count().await, 1);
    assert!(!lc.is_loaded("a").await);
    assert!(lc.is_loaded("b").await);
}

#[tokio::test]
async fn validate_handle_returns_true_for_loaded_model() {
    let lc = make_lifecycle();
    let model = MockModel::new("valid", 512);
    let meta = make_metadata("valid", 512);

    let handle = lc.load("valid".into(), meta, model).await.unwrap();
    assert!(lc.validate_handle(&handle).await);
}

#[tokio::test]
async fn validate_handle_returns_false_for_unloaded_model() {
    let lc = make_lifecycle();
    let model = MockModel::new("temp", 256);
    let meta = make_metadata("temp", 256);

    let handle = lc.load("temp".into(), meta, model).await.unwrap();
    lc.unload("temp").await.unwrap();
    assert!(!lc.validate_handle(&handle).await);
}

#[tokio::test]
async fn validate_handle_returns_false_for_bogus_handle() {
    let lc = make_lifecycle();
    let bogus = ModelHandle::new(9999);
    assert!(!lc.validate_handle(&bogus).await);
}

#[tokio::test]
async fn concurrent_load_same_model_exactly_one_wins() {
    let lc = Arc::new(make_lifecycle());
    let n = 10;

    let handles: Vec<_> = (0..n)
        .map(|i| {
            let lc = Arc::clone(&lc);
            tokio::spawn(async move {
                let model = MockModel::new("race", 100 + i);
                let meta = make_metadata("race", 100);
                lc.load("race".into(), meta, model).await
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    let successes: Vec<_> = results.iter().filter(|r| r.is_ok()).collect();
    let failures: Vec<_> = results.iter().filter(|r| r.is_err()).collect();

    assert_eq!(successes.len(), 1, "exactly one load must succeed");
    assert_eq!(failures.len(), n - 1, "all others must get AlreadyLoaded");
    assert_eq!(lc.count().await, 1);
}
