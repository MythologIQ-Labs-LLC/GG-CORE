//! Lifecycle resident-weight accounting: weights are reserved once at load
//! against the total budget and released on unload.

use std::sync::Arc;

use super::*;
use crate::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError as EngineError,
    InferenceInput, InferenceOutput, Model,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::loader::ModelMetadata;

struct Weights(usize);

#[async_trait::async_trait]
impl Model for Weights {
    fn model_id(&self) -> &str {
        "w"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        self.0
    }
    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, EngineError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: String::new(),
            tokens_generated: 0,
            finish_reason: crate::engine::FinishReason::MaxTokens,
        }))
    }
    async fn unload(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn setup(total: usize) -> (ModelLifecycle, ResourceLimits) {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 10,
        max_total_memory: total,
        max_concurrent: 2,
    });
    let registry = Arc::new(ModelRegistry::new());
    let engine = Arc::new(InferenceEngine::new(4096));
    let lc = ModelLifecycle::new(registry, engine).with_resource_limits(limits.clone());
    (lc, limits)
}

fn meta(name: &str, size: u64) -> ModelMetadata {
    ModelMetadata {
        name: name.into(),
        size_bytes: size,
    }
}

async fn load(lc: &ModelLifecycle, id: &str, bytes: usize) -> Result<ModelHandle, LifecycleError> {
    lc.load(id.into(), meta(id, bytes as u64), Arc::new(Weights(bytes)))
        .await
}

#[tokio::test]
async fn load_reserves_resident_weights_larger_than_per_call() {
    let (lc, limits) = setup(1_000);
    load(&lc, "big", 600)
        .await
        .expect("600 fits the 1000 total");
    assert_eq!(limits.current_memory(), 600, "weights reserved once");
}

#[tokio::test]
async fn load_beyond_total_is_refused_and_not_registered() {
    let (lc, limits) = setup(1_000);
    load(&lc, "a", 700).await.unwrap();
    let err = load(&lc, "b", 400).await.unwrap_err();
    assert!(matches!(err, LifecycleError::ResourceExhausted(_)), "{err}");
    assert!(
        err.to_string().contains("GG_CORE_MAX_TOTAL_MEMORY"),
        "{err}"
    );
    assert!(
        !lc.is_loaded("b").await,
        "refused model must not be visible"
    );
    assert_eq!(limits.current_memory(), 700, "refused load must not leak");
}

#[tokio::test]
async fn unload_releases_resident_and_allows_reload_or_swap() {
    let (lc, limits) = setup(1_000);
    load(&lc, "a", 700).await.unwrap();
    lc.unload("a").await.unwrap();
    assert_eq!(
        limits.current_memory(),
        0,
        "unload releases the reservation"
    );
    load(&lc, "b", 900)
        .await
        .expect("swap to a larger model after unload");
    lc.unload("b").await.unwrap();
    load(&lc, "a", 700).await.expect("reload");
    assert_eq!(limits.current_memory(), 700);
}

#[tokio::test]
async fn working_reservations_and_residents_share_one_budget() {
    let (lc, limits) = setup(1_000);
    load(&lc, "a", 985).await.unwrap();
    let _working = limits.try_acquire(10).expect("985 + 10 fits");
    assert!(
        limits.try_acquire(10).is_err(),
        "985 + 10 + 10 exceeds 1000"
    );
}

#[test]
fn oversized_load_is_refused_before_backend_io() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    // Not a valid GGUF: reaching the backend would fail with a backend error.
    std::fs::write(dir.path().join("models/big.gguf"), vec![0u8; 4096]).unwrap();
    let loader = crate::models::ModelLoader::new(dir.path().to_path_buf());
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 10,
        max_total_memory: 1024,
        max_concurrent: 1,
    });
    let result =
        crate::models::prepare_model_admitted(&loader, Some(&limits), "models/big.gguf", None);
    let err = result
        .err()
        .expect("4096-byte model must not fit a 1024 budget");
    assert!(
        matches!(
            err,
            crate::models::ModelOpError::Lifecycle(LifecycleError::ResourceExhausted(_))
        ),
        "must be refused by admission, not by the backend: {err}"
    );
}
