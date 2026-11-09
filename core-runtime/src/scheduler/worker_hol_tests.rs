//! Head-of-line blocking reproduction (plan:
//! `docs/integration/V1_SCHEDULER_HOL_BLOCKING_PLAN.md`).
//!
//! The daemon runs one serial worker. A streaming request whose consumer
//! stops reading (IPC pipe back-pressure fills the token channel) parks that
//! worker inside an unbounded `TokenStreamSender` send, so every other
//! request waits behind it, past the stream's own deadline. No model is
//! needed: the worker's first send to the stalled consumer already blocks,
//! with or without the `gguf` feature.

use std::sync::Arc;
use std::time::Duration;

use super::spawn_worker;
use crate::engine::inference::InferenceEngine;
use crate::engine::{InferenceConfig, InferenceParams, TokenStream};
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig, ResponseRx};
use crate::scheduler::Priority;

/// Stream deadline far shorter than how long the test waits behind it.
const STREAM_TIMEOUT_MS: u64 = 300;
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
/// The returned receiver must stay alive for the stall to persist.
async fn enqueue_stalled_stream(queue: &RequestQueue) -> TokenStream {
    let (tx, rx) = TokenStream::new(1);
    tx.token(0).await.expect("prefill the only slot");
    let config = InferenceConfig {
        timeout_ms: STREAM_TIMEOUT_MS,
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

/// Characterization of today's behavior (passes on the unfixed scheduler).
/// When the fix lands this test must fail; replace it with the ignored
/// test below.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_stream_consumer_blocks_unrelated_request_today() {
    let h = start();
    let stalled = enqueue_stalled_stream(&h.queue).await;
    // Let the worker take the stream and park on the full channel.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut unrelated = enqueue_unrelated(&h.queue).await;

    // Wait well past the stream's own deadline: still no answer.
    let waited = tokio::time::timeout(PROMPT_ANSWER, &mut unrelated).await;
    assert!(
        waited.is_err(),
        "HOL blocking no longer reproduces: switch to the ignored fixed-behavior test"
    );

    // Only the consumer going away releases the worker.
    drop(stalled);
    let answer = tokio::time::timeout(PROMPT_ANSWER, unrelated)
        .await
        .expect("worker released once the stalled consumer was dropped")
        .expect("response sender kept");
    assert!(answer.is_err(), "absent model must be an explicit error");
    stop(h).await;
}

/// Required behavior after the fix: an unrelated request is answered while
/// a stream consumer is stalled, and the stalled stream is ended by its own
/// deadline rather than by the consumer disconnecting.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "fails until the HOL-blocking fix lands (V1_SCHEDULER_HOL_BLOCKING_PLAN)"]
async fn stalled_stream_consumer_does_not_block_unrelated_request() {
    let h = start();
    let _stalled = enqueue_stalled_stream(&h.queue).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let unrelated = enqueue_unrelated(&h.queue).await;

    let answer = tokio::time::timeout(PROMPT_ANSWER, unrelated)
        .await
        .expect("unrelated request starved behind a stalled stream consumer")
        .expect("response sender kept");
    assert!(answer.is_err(), "absent model must be an explicit error");
    stop(h).await;
}

async fn stop(h: Harness) {
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(1), h.worker).await;
}
