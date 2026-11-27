//! Admission fence semantics on the engine alone (no lifecycle, no budget).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError as BackendError,
    InferenceInput, InferenceOutput,
};
use crate::models::ModelHandle;

struct Stub;

#[async_trait::async_trait]
impl Model for Stub {
    fn model_id(&self) -> &str {
        "stub"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        1
    }
    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, BackendError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: String::new(),
            tokens_generated: 0,
            finish_reason: crate::engine::FinishReason::MaxTokens,
        }))
    }
    async fn unload(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

async fn engine_with_stub() -> Arc<InferenceEngine> {
    let engine = Arc::new(InferenceEngine::new(4096));
    engine
        .register_model("m".into(), ModelHandle::new(1), Arc::new(Stub))
        .await;
    engine
}

#[tokio::test]
async fn unfenced_admission_is_immediate_and_counts_as_a_lease() {
    let engine = engine_with_stub().await;
    assert_eq!(
        engine.strong_count("m").await,
        Some(1),
        "store reference only"
    );
    let lease = engine.admit("m", None).await.expect("admitted");
    assert_eq!(engine.strong_count("m").await, Some(2));
    drop(lease);
    assert_eq!(engine.strong_count("m").await, Some(1));
    assert!(matches!(
        engine.admit("absent", None).await,
        Err(InferenceError::ModelNotLoaded(_))
    ));
}

#[tokio::test]
async fn fence_counts_leases_atomically_and_refuses_a_second_fence() {
    let engine = engine_with_stub().await;
    let lease = engine.admit("m", None).await.expect("admitted");
    let (fence, count) = engine.fence_admissions("m").await.expect("fenced");
    assert_eq!(count, 2, "the store's reference plus one lease");
    assert!(
        engine.fence_admissions("m").await.is_none(),
        "already fenced"
    );
    assert!(engine.fence_admissions("absent").await.is_none());
    drop(lease);
    assert_eq!(engine.strong_count("m").await, Some(1));
    drop(fence);
    assert!(engine.fence_admissions("m").await.is_some(), "fence lifted");
}

#[tokio::test(start_paused = true)]
async fn fenced_admission_waits_without_a_lease_until_the_fence_drops() {
    let engine = engine_with_stub().await;
    let (fence, _) = engine.fence_admissions("m").await.expect("fenced");
    let waiter = {
        let engine = Arc::clone(&engine);
        tokio::spawn(async move { engine.admit("m", None).await.map(|_| ()) })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!waiter.is_finished(), "must wait while fenced");
    assert_eq!(
        engine.strong_count("m").await,
        Some(1),
        "a waiter holds no lease"
    );
    drop(fence);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(waiter.is_finished(), "woken by the dropped fence");
    waiter
        .await
        .expect("joined")
        .expect("admitted after the fence");
}

#[tokio::test(start_paused = true)]
async fn fenced_admission_honours_deadline_and_cancellation() {
    let engine = engine_with_stub().await;
    let (_fence, _) = engine.fence_admissions("m").await.expect("fenced");
    let expired = StopControl::from_timeout(None, Some(50));
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(matches!(
        engine.admit("m", Some(&expired)).await,
        Err(InferenceError::DeadlineExceeded { timeout_ms: 50 })
    ));
    let flag = Arc::new(AtomicBool::new(false));
    let cancelled = StopControl::from_timeout(Some(Arc::clone(&flag)), None);
    let waiter = {
        let engine = Arc::clone(&engine);
        tokio::spawn(async move { engine.admit("m", Some(&cancelled)).await.map(|_| ()) })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiter.is_finished());
    flag.store(true, Ordering::Release);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(matches!(
        waiter.await.expect("joined"),
        Err(InferenceError::Cancelled)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_admission_is_bounded_by_its_stop_check() {
    let engine = engine_with_stub().await;
    let (fence, _) = engine.fence_admissions("m").await.expect("fenced");
    let stop = Arc::new(AtomicBool::new(false));
    let (e, s) = (Arc::clone(&engine), Arc::clone(&stop));
    let stopped = tokio::task::spawn_blocking(move || {
        let check = move || s.load(Ordering::Acquire);
        e.admit_blocking("m", Some(&check)).map(|_| ())
    });
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!stopped.is_finished(), "waits while fenced");
    stop.store(true, Ordering::Release);
    let started = Instant::now();
    assert!(matches!(
        stopped.await.expect("joined"),
        Err(InferenceError::ExecutionFailed(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(fence);
    let e = Arc::clone(&engine);
    let admitted = tokio::task::spawn_blocking(move || e.admit_blocking("m", None).map(|_| ()));
    admitted
        .await
        .expect("joined")
        .expect("admitted once unfenced");
}
