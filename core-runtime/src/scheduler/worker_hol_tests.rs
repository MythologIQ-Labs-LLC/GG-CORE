//! Head-of-line blocking regression (plan:
//! `docs/integration/V1_SCHEDULER_HOL_BLOCKING_PLAN.md`).
//!
//! The daemon runs one serial worker. Before Phase 1, a streaming request
//! whose consumer stopped reading (IPC pipe back-pressure fills the token
//! channel) parked that worker inside an unbounded `TokenStreamSender` send,
//! so every other request waited behind it until the consumer disconnected.
//! Every send is now bounded by the stream's stall bound. No model is needed:
//! the worker's first send to the stalled consumer already blocks, with or
//! without the `gguf` feature.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::spawn_worker;
use crate::engine::inference::InferenceEngine;
use crate::engine::{InferenceConfig, InferenceParams, StreamBound, TokenStream};
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig, ResponseRx};
use crate::scheduler::Priority;

/// Stall bound of the stalled stream.
const STALL: Duration = Duration::from_millis(200);
/// Stream deadline, much longer than the stall bound.
const STREAM_TIMEOUT: Duration = Duration::from_secs(30);
/// How long an unrelated request is given to be answered.
const PROMPT_ANSWER: Duration = Duration::from_secs(2);

struct Harness {
    queue: Arc<RequestQueue>,
    shutdown: tokio_util::sync::CancellationToken,
    worker: tokio::task::JoinHandle<()>,
}

fn start() -> Harness {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig {
        max_pending: 8,
        ..Default::default()
    }));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let engine = Arc::new(InferenceEngine::new(4096));
    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());
    Harness {
        queue,
        shutdown,
        worker,
    }
}

/// Enqueue a stream whose consumer never reads: capacity 1, already full.
/// The returned receiver stays alive, so only the bound can free the worker.
async fn enqueue_stalled_stream(queue: &RequestQueue) -> TokenStream {
    let bound = StreamBound::for_request(STALL, STREAM_TIMEOUT);
    let (tx, rx) = TokenStream::new(1, bound);
    tx.token(0).await.expect("prefill the only slot");
    let config = InferenceConfig {
        timeout_ms: STREAM_TIMEOUT.as_millis() as u64,
        ..Default::default()
    };
    queue
        .enqueue_streaming("absent-model".into(), "hi".into(), config, tx)
        .await
        .expect("enqueue stalled stream");
    rx
}

async fn enqueue_unrelated(queue: &RequestQueue) -> ResponseRx {
    let (_id, rx) = queue
        .enqueue_with_response(
            "absent-model".into(),
            "hello".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .expect("enqueue unrelated request");
    rx
}

/// An unrelated request is answered while a stream consumer is stalled,
/// within the stall bound rather than the stream deadline or a disconnect.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_stream_consumer_does_not_block_unrelated_request() {
    let h = start();
    let _stalled = enqueue_stalled_stream(&h.queue).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let start = Instant::now();
    let unrelated = enqueue_unrelated(&h.queue).await;

    let answer = tokio::time::timeout(PROMPT_ANSWER, unrelated)
        .await
        .expect("unrelated request starved behind a stalled stream consumer")
        .expect("response sender kept");
    assert!(answer.is_err(), "absent model must be an explicit error");
    assert!(
        start.elapsed() < STREAM_TIMEOUT / 10,
        "freed by the stall bound"
    );
    stop(h).await;
}

/// The stalled stream is lost (never reported complete) and the worker keeps
/// serving: a second stalled stream followed by a request is also bounded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_stalled_consumers_each_hold_the_worker_only_for_the_bound() {
    let h = start();
    let _a = enqueue_stalled_stream(&h.queue).await;
    let _b = enqueue_stalled_stream(&h.queue).await;
    let unrelated = enqueue_unrelated(&h.queue).await;
    let answer = tokio::time::timeout(PROMPT_ANSWER, unrelated)
        .await
        .expect("two stalled consumers must cost at most two stall bounds")
        .expect("response sender kept");
    assert!(answer.is_err());
    stop(h).await;
}

async fn stop(h: Harness) {
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(1), h.worker).await;
}
