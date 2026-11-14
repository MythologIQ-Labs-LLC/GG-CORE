//! #184 stop trace in the worker: backend-side events on the blocking
//! thread carry the request's `queue_id` (span), and S4 reports the release
//! with the same id. Queue ids restart at 1 per queue, so the capture is
//! keyed by a unique marker the test model emits, not by the id alone.

use super::*;
use crate::telemetry::stop_trace::test_capture;

const MARKER: u64 = 18_410;

/// Emits a marked `gg_core::stop` event on the blocking thread when it sees
/// the stop, then ends cancelled.
struct MarkedModel;

#[async_trait::async_trait]
impl Model for MarkedModel {
    fn model_id(&self) -> &str {
        "marked"
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
        _: &InferenceInput,
        _: &InferenceConfig,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, EngineError> {
        for _ in 0..500 {
            if is_cancelled.is_some_and(|f| f()) {
                tracing::info!(target: "gg_core::stop", marker = MARKER, "marked stop seen");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "secret output".into(),
            tokens_generated: 1,
            finish_reason: FinishReason::Cancelled,
        }))
    }
    async fn unload(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_thread_events_and_release_share_the_queue_id() {
    test_capture::install();
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let engine = Arc::new(InferenceEngine::new(4096));
    let model: Arc<dyn Model> = Arc::new(MarkedModel);
    engine
        .register_model("marked".into(), crate::models::ModelHandle::new(1), model)
        .await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    drop(spawn_worker(Arc::clone(&queue), engine, shutdown.clone()));
    let flag = Arc::new(AtomicBool::new(false));
    let (queue_id, rx) = queue
        .enqueue_controlled(
            "marked".into(),
            "secret prompt".into(),
            params(None),
            Priority::Normal,
            Arc::clone(&flag),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    flag.store(true, Ordering::Release);
    let _ = answer(rx).await.expect_err("cancelled");

    let events = test_capture::events();
    let at = events
        .iter()
        .position(|e| e["fields"]["marker"] == MARKER)
        .expect("marked event captured from the blocking thread");
    assert_eq!(
        events[at]["span"]["queue_id"], queue_id,
        "span reaches the blocking thread"
    );
    assert_eq!(events[at]["span"]["stream"], false);
    // Queue ids restart at 1 per queue: match every field this request's S4
    // must carry, after its own backend observation (review C3).
    let released = events[at..].iter().any(|e| {
        let f = &e["fields"];
        f["message"] == "worker released after stop"
            && f["queue_id"] == queue_id
            && f["stream"] == false
            && f["cancel_flag"] == true
    });
    assert!(released, "S4 after the backend observation");
    let raw = test_capture::raw();
    assert!(!raw.contains("secret prompt") && !raw.contains("secret output"));
    shutdown.cancel();
}

#[test]
fn a_successful_result_emits_no_release_event() {
    test_capture::install();
    let ok: InferResult = Ok(crate::engine::InferenceResult {
        output: "secret output".into(),
        tokens_generated: 1,
        finished: true,
    });
    trace_stop_release(&ok, &crate::engine::StopControl::default(), 9_184_001, 5);
    let released = test_capture::events()
        .into_iter()
        .any(|e| e["fields"]["queue_id"] == 9_184_001);
    assert!(!released);
}
