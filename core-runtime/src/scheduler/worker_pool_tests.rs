//! Phase 2: `max_concurrent` worker loops, bounded by the same admission
//! guard, fair across queues, and joined on shutdown.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engine::inference::InferenceEngine;
use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig, InferenceError,
    InferenceInput, InferenceOutput, InferenceParams, Model, StreamBound, TokenStream,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::ModelHandle;
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig, ResponseRx};
use crate::scheduler::{spawn_worker_with_registry, Priority};

const WORK: Duration = Duration::from_millis(300);

/// Takes `WORK` per request and records the peak number in flight.
struct SlowModel {
    in_flight: AtomicUsize,
    peak: Arc<AtomicUsize>,
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
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(WORK).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "ok".into(),
            tokens_generated: 1,
            finish_reason: FinishReason::Stop,
        }))
    }
    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct Pool {
    queue: Arc<RequestQueue>,
    peak: Arc<AtomicUsize>,
    shutdown: tokio_util::sync::CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

async fn pool(max_concurrent: usize) -> Pool {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig {
        max_pending: 64,
        ..Default::default()
    }));
    spawn_on(queue, max_concurrent).await
}

async fn spawn_on(queue: Arc<RequestQueue>, max_concurrent: usize) -> Pool {
    let peak = Arc::new(AtomicUsize::new(0));
    let model = SlowModel {
        in_flight: AtomicUsize::new(0),
        peak: peak.clone(),
    };
    let engine = Arc::new(InferenceEngine::new(4096));
    engine
        .register_model("slow".into(), ModelHandle::new(1), Arc::new(model))
        .await;
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024,
        max_total_memory: 64 * 1024 * 1024,
        max_concurrent,
    });
    let shutdown = tokio_util::sync::CancellationToken::new();
    let handle = spawn_worker_with_registry(
        queue.clone(),
        engine,
        None,
        None,
        Some(limits),
        None,
        shutdown.clone(),
    );
    Pool {
        queue,
        peak,
        shutdown,
        handle,
    }
}

async fn request(queue: &RequestQueue) -> ResponseRx {
    let params = InferenceParams::default();
    let (_id, rx) = queue
        .enqueue_with_response("slow".into(), "hi".into(), params, Priority::Normal)
        .await
        .expect("enqueue");
    rx
}

async fn answered(rx: ResponseRx) {
    let result = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("answer deadline")
        .expect("sender kept");
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_loops_run_two_requests_at_the_same_time() {
    let p = pool(2).await;
    let start = Instant::now();
    let (a, b) = (request(&p.queue).await, request(&p.queue).await);
    answered(a).await;
    answered(b).await;
    assert!(
        start.elapsed() < WORK * 2 - WORK / 4,
        "requests did not overlap"
    );
    assert_eq!(p.peak.load(Ordering::SeqCst), 2);
    p.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn in_flight_never_exceeds_max_concurrent() {
    let p = pool(2).await;
    let mut pending = Vec::new();
    for _ in 0..6 {
        pending.push(request(&p.queue).await);
    }
    for rx in pending {
        answered(rx).await;
    }
    assert_eq!(p.peak.load(Ordering::SeqCst), 2);
    p.shutdown.cancel();
}

/// With one loop, queued streams no longer starve a queued main request:
/// the rotation serves it between streams. Strict stream-first order would
/// end all three streams before the (slow) main request starts.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_do_not_starve_main_requests() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let mut streams = Vec::new();
    for _ in 0..3 {
        let (tx, rx) = TokenStream::new(4, StreamBound::default());
        queue
            .enqueue_streaming("absent".into(), "hi".into(), InferenceConfig::default(), tx)
            .await
            .expect("enqueue stream");
        streams.push(rx);
    }
    let main = request(&queue).await;
    let last = streams.pop().expect("three streams");
    // Streams to an absent model end at once; the main request takes WORK.
    // Strict stream-first order ends the last stream near `start`; rotation
    // ends it after the main request, about WORK later. Comparing against
    // WORK / 2, not the main answer's wake-up instant, is immune to task
    // wake-up jitter between the two observers.
    let start = Instant::now();
    let p = spawn_on(queue, 1).await;
    let last_done = tokio::spawn(async move {
        let _ = last.collect().await;
        Instant::now()
    });
    answered(main).await;
    let last_done = last_done.await.expect("stream joined");
    assert!(
        last_done.duration_since(start) >= WORK / 2,
        "main request waited behind every stream"
    );
    p.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_joins_every_loop() {
    let p = pool(3).await;
    p.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), p.handle)
        .await
        .expect("every loop observes shutdown")
        .expect("pool joined");
}
