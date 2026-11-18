//! Working-memory admission: estimate-first reservation, fallback, ceiling.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError as EngineError,
    InferenceInput, InferenceOutput, Model,
};
use crate::memory::ResourceLimitsConfig;
use crate::models::ModelHandle;
use crate::security::{SecurityConfig, SecurityPipeline};

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

#[tokio::test]
async fn unboundable_model_is_refused_explicitly_without_reserving() {
    let (engine, l) = (engine_with(Some(usize::MAX)).await, limits(100, usize::MAX));
    let err = acquire_guard(&engine, Some(&l), "m").await.unwrap_err();
    assert_eq!(err, UNBOUNDED);
    assert_eq!((l.current_memory(), l.current_concurrent()), (0, 0));
}

/// Model-free regression of the real shared blocking-admission boundary.
/// The blocked native task owns its reservation and both Arcs even after its
/// Tokio awaiter is aborted. A fake/synthetic inference response is not proof
/// of a GGUF/ONNX backend run; this tests the scheduler lifetime contract.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_waiter_cannot_release_running_native_reservation() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 64,
        max_total_memory: 64,
        max_concurrent: 1,
    });
    let guard = limits.try_acquire_working(64).expect("first admission");
    let engine = Arc::new(InferenceEngine::new(4096));
    let security = Arc::new(SecurityPipeline::from_config(&SecurityConfig::default()));
    let closure_engine = Arc::clone(&engine);
    let closure_security = Arc::clone(&security);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let blocked = Arc::clone(&gate);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();

    let waiter = tokio::spawn(async move {
        run_blocking_admitted(Some(guard), move || {
            // The real stream implementation also owns these Arc handles.
            let (_engine, _security) = (closure_engine, closure_security);
            let _ = started_tx.send(());
            let (lock, cv) = &*blocked;
            let released = lock.lock().expect("lock gate");
            // Time bound prevents a failed test from stranding a blocking task.
            let (released, _) = cv
                .wait_timeout_while(released, Duration::from_secs(10), |v| !*v)
                .expect("wait gate");
            assert!(*released, "gate was not released");
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .expect("blocking closure did not start")
        .expect("start signal dropped");
    waiter.abort();
    assert!(waiter
        .await
        .expect_err("awaiting task aborted")
        .is_cancelled());

    assert_eq!(limits.current_memory(), 64);
    assert_eq!(limits.current_concurrent(), 1);
    assert!(
        limits.try_acquire_working(1).is_err(),
        "an abandoned awaiter must not admit overlapping native work"
    );
    assert!(
        Arc::strong_count(&engine) > 1,
        "native engine owner was lost"
    );
    assert!(
        Arc::strong_count(&security) > 1,
        "native security pipeline owner was lost"
    );

    {
        let (lock, cv) = &*gate;
        *lock.lock().expect("release lock") = true;
        cv.notify_all();
    }
    let start = Instant::now();
    while limits.current_concurrent() != 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "native task retained admission after exiting"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(Arc::strong_count(&engine), 1);
    assert_eq!(Arc::strong_count(&security), 1);
    let second = limits
        .try_acquire_working(64)
        .expect("slot released after exit");
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (64, 1)
    );
    drop(second);
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (0, 0)
    );
}

/// Panic is another exit path: the RAII reservation must be released once
/// even when a native callback unwinds into a Tokio JoinError.
#[tokio::test]
async fn native_panic_releases_admission_without_a_leak() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 64,
        max_total_memory: 64,
        max_concurrent: 1,
    });
    let guard = limits.try_acquire_working(64).unwrap();
    let err = run_blocking_admitted(Some(guard), || -> () {
        panic!("intentional native-task test panic");
    })
    .await
    .expect_err("panic must reach the join handle");
    assert!(err.is_panic());
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (0, 0)
    );
    assert!(
        limits.try_acquire_working(64).is_ok(),
        "admission leaked after panic"
    );
}
