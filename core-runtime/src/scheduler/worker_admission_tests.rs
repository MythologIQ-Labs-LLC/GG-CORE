//! Working-memory admission: estimate-first reservation, fallback, ceiling.

use std::sync::Arc;

use super::*;
use crate::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError as EngineError,
    InferenceInput, InferenceOutput, Model,
};
use crate::memory::ResourceLimitsConfig;
use crate::models::ModelHandle;

struct Sized(Option<usize>);

#[async_trait::async_trait]
impl Model for Sized {
    fn model_id(&self) -> &str {
        "m"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        0
    }
    fn working_memory_estimate(&self) -> Option<usize> {
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

async fn engine_with(estimate: Option<usize>) -> InferenceEngine {
    let engine = InferenceEngine::new(4096);
    let handle = ModelHandle::new(1);
    engine
        .register_model("m".into(), handle, Arc::new(Sized(estimate)))
        .await;
    engine
}

fn limits(per_call: usize, total: usize) -> ResourceLimits {
    ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: per_call,
        max_total_memory: total,
        max_concurrent: 4,
    })
}

#[tokio::test]
async fn reserves_the_model_estimate_not_the_fallback() {
    let (engine, l) = (engine_with(Some(5_000)).await, limits(100, 10_000));
    let guard = acquire_guard(&engine, Some(&l), "m").await.unwrap();
    assert!(guard.is_some());
    assert_eq!(
        l.current_memory(),
        5_000,
        "estimate above the fallback is reserved"
    );
    drop(guard);
    assert_eq!(l.current_memory(), 0);
}

#[tokio::test]
async fn falls_back_to_configured_reservation_without_estimate() {
    let (engine, l) = (engine_with(None).await, limits(100, 10_000));
    let _g = acquire_guard(&engine, Some(&l), "m").await.unwrap();
    assert_eq!(l.current_memory(), 100);
}

#[tokio::test]
async fn estimate_beyond_total_is_refused_without_side_effects() {
    let (engine, l) = (engine_with(Some(20_000)).await, limits(100, 10_000));
    let err = acquire_guard(&engine, Some(&l), "m").await.unwrap_err();
    assert!(err.contains("GG_CORE_MAX_TOTAL_MEMORY"), "{err}");
    assert_eq!((l.current_memory(), l.current_concurrent()), (0, 0));
}

#[tokio::test]
async fn explicit_ceiling_below_estimate_is_refused_and_named() {
    let (engine, l) = (engine_with(Some(5_000)).await, limits(100, 10_000));
    l.set_per_call_ceiling(4_000).unwrap();
    let err = acquire_guard(&engine, Some(&l), "m").await.unwrap_err();
    assert!(err.contains("GG_CORE_MAX_MEMORY_PER_CALL"), "{err}");
    assert_eq!(l.current_memory(), 0);
}

#[tokio::test]
async fn two_requests_reserve_twice_the_estimate_within_total() {
    let (engine, l) = (engine_with(Some(4_000)).await, limits(100, 10_000));
    let _a = acquire_guard(&engine, Some(&l), "m").await.unwrap();
    let _b = acquire_guard(&engine, Some(&l), "m").await.unwrap();
    assert_eq!(l.current_memory(), 8_000);
    assert!(
        acquire_guard(&engine, Some(&l), "m").await.is_err(),
        "third exceeds total"
    );
}

#[tokio::test]
async fn no_limits_means_no_guard() {
    let engine = engine_with(Some(5_000)).await;
    assert!(acquire_guard(&engine, None, "m").await.unwrap().is_none());
}
