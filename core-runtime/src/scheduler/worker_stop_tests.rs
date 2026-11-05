//! Non-streaming execution must stop for cancellation and deadlines while the
//! model is running, and the caller must get an explicit failure even when a
//! backend ignores the checkpoint and returns output after the stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::engine::inference::InferenceEngine;
use crate::engine::Model;
use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig,
    InferenceError as EngineError, InferenceInput, InferenceOutput, InferenceParams,
};
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig};
use crate::scheduler::Priority;

/// Simulates a long prefill: 200 steps of 10 ms with a checkpoint per step.
/// `honor_stop: false` models a backend that ignores the checkpoint.
struct SlowModel {
    honor_stop: bool,
}

#[async_trait::async_trait]
impl Model for SlowModel {
    fn model_id(&self) -> &str {
        "slow"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        1024
    }
    async fn infer(
        &self,
        input: &InferenceInput,
        config: &InferenceConfig,
    ) -> Result<InferenceOutput, EngineError> {
        self.infer_cancellable(input, config, None).await
    }
    async fn infer_cancellable(
        &self,
        input: &InferenceInput,
        _: &InferenceConfig,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, EngineError> {
        let quick = matches!(input, InferenceInput::Text(p) if p == "quick");
        // A backend that honors the checkpoint stops within a few steps, so
        // its nominal length only has to exceed the test deadlines. The
        // ignoring backend runs to completion: keep that well past the 50 ms
        // deadline but short enough that a loaded 3-vCPU CI runner (macOS
        // overshot a 5 s budget with 200 steps) cannot turn it into a timeout.
        let steps = if quick {
            1
        } else if self.honor_stop {
            200
        } else {
            40
        };
        let mut finish = FinishReason::Stop;
        for _ in 0..steps {
            if self.honor_stop && is_cancelled.is_some_and(|f| f()) {
                finish = FinishReason::Cancelled;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "partial output".into(),
            tokens_generated: 3,
            finish_reason: finish,
        }))
    }
    async fn unload(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

async fn setup(honor_stop: bool) -> (Arc<RequestQueue>, tokio_util::sync::CancellationToken) {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let engine = Arc::new(InferenceEngine::new(4096));
    let model: Arc<dyn Model> = Arc::new(SlowModel { honor_stop });
    engine
        .register_model("slow".into(), crate::models::ModelHandle::new(1), model)
        .await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    // Detached: the worker runs until `shutdown` is cancelled.
    drop(spawn_worker(Arc::clone(&queue), engine, shutdown.clone()));
    (queue, shutdown)
}

fn params(timeout_ms: Option<u64>) -> InferenceParams {
    InferenceParams {
        timeout_ms,
        ..Default::default()
    }
}

async fn answer(
    rx: crate::scheduler::ResponseRx,
) -> Result<crate::engine::InferenceResult, String> {
    // Generous: promptness is asserted separately (< 1 s) where it matters;
    // this bound only guards against a hung worker on a slow runner.
    tokio::time::timeout(Duration::from_secs(30), rx)
        .await
        .expect("worker must answer")
        .expect("response channel must not be dropped")
}

#[tokio::test]
async fn deadline_stops_running_request_with_explicit_error() {
    let (queue, shutdown) = setup(true).await;
    let start = Instant::now();
    let (_, rx) = queue
        .enqueue_with_response(
            "slow".into(),
            "long".into(),
            params(Some(100)),
            Priority::Normal,
        )
        .await
        .unwrap();
    let err = answer(rx).await.expect_err("expired request must fail");
    assert!(err.contains("Deadline exceeded"), "{err}");
    assert!(
        start.elapsed() < Duration::from_millis(1000),
        "{:?}",
        start.elapsed()
    );

    // The worker is free again: the next request completes.
    let (_, rx) = queue
        .enqueue_with_response(
            "slow".into(),
            "quick".into(),
            params(None),
            Priority::Normal,
        )
        .await
        .unwrap();
    assert_eq!(answer(rx).await.expect("recovery").output, "partial output");
    shutdown.cancel();
}

#[tokio::test]
async fn cancel_flag_stops_running_request_with_explicit_error() {
    let (queue, shutdown) = setup(true).await;
    let flag = Arc::new(AtomicBool::new(false));
    let (_, rx) = queue
        .enqueue_controlled(
            "slow".into(),
            "long".into(),
            params(None),
            Priority::Normal,
            Arc::clone(&flag),
        )
        .await
        .unwrap();
    let setter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flag.store(true, Ordering::Release);
    });
    let start = Instant::now();
    let err = answer(rx).await.expect_err("cancelled request must fail");
    setter.join().unwrap();
    assert!(err.contains("Request cancelled"), "{err}");
    assert!(
        start.elapsed() < Duration::from_millis(1000),
        "{:?}",
        start.elapsed()
    );
    shutdown.cancel();
}

#[tokio::test]
async fn backend_ignoring_stop_still_yields_explicit_failure() {
    let (queue, shutdown) = setup(false).await;
    let (_, rx) = queue
        .enqueue_with_response(
            "slow".into(),
            "long".into(),
            params(Some(50)),
            Priority::Normal,
        )
        .await
        .unwrap();
    let err = answer(rx)
        .await
        .expect_err("late output must not be a success");
    assert!(err.contains("Deadline exceeded"), "{err}");
    shutdown.cancel();
}
