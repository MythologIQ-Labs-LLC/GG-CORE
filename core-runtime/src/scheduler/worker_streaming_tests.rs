//! Regression tests for streaming requests reaching an already-idle worker
//! (RC acceptance finding: a streaming request enqueued after the worker
//! parked was only serviced once unrelated non-streaming traffic arrived).

use std::sync::Arc;
use std::time::Duration;

use super::spawn_worker;
use crate::engine::inference::InferenceEngine;
use crate::engine::{InferenceConfig, StreamTerminal, TokenStream};
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig};

#[tokio::test]
async fn streaming_request_is_serviced_by_an_idle_worker() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig {
        max_pending: 8,
        ..Default::default()
    }));
    let engine = Arc::new(InferenceEngine::new(4096));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());

    // Let the worker reach its idle wait before any streaming work exists.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (tx, rx) = TokenStream::new(8);
    queue
        .enqueue_streaming(
            "absent-model".into(),
            "hi".into(),
            InferenceConfig::default(),
            tx,
        )
        .await
        .unwrap();

    // No model is loaded (and no gguf backend may be compiled in), so the
    // request must terminate with an error — promptly, not after the next
    // non-streaming request happens to wake the worker.
    let (_tokens, terminal) = tokio::time::timeout(Duration::from_secs(3), rx.collect())
        .await
        .expect("streaming request was never serviced by the idle worker");
    match terminal {
        StreamTerminal::Error(msg) => {
            assert!(
                !msg.contains("stream dropped"),
                "worker dropped the stream: {msg}"
            )
        }
        other => panic!("expected an Error terminal, got {other:?}"),
    }

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn cancelled_queued_stream_reports_error_terminal() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let (tx, rx) = TokenStream::new(8);
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    queue
        .enqueue_streaming_controlled(
            "absent-model".into(),
            "hi".into(),
            InferenceConfig::default(),
            tx,
            cancel,
        )
        .await
        .unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = spawn_worker(
        queue,
        Arc::new(InferenceEngine::new(4096)),
        shutdown.clone(),
    );
    let (_tokens, terminal) = tokio::time::timeout(Duration::from_secs(2), rx.collect())
        .await
        .expect("cancelled request must receive terminal");
    assert!(
        matches!(terminal, StreamTerminal::Error(ref msg) if msg.contains("cancelled")),
        "cancelled stream must not report successful completion"
    );
    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn expired_queued_stream_reports_error_terminal() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let (tx, rx) = TokenStream::new(8);
    let config = InferenceConfig {
        timeout_ms: 1,
        ..Default::default()
    };
    queue
        .enqueue_streaming("absent-model".into(), "hi".into(), config, tx)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = spawn_worker(
        queue,
        Arc::new(InferenceEngine::new(4096)),
        shutdown.clone(),
    );
    let (_tokens, terminal) = tokio::time::timeout(Duration::from_secs(2), rx.collect())
        .await
        .expect("expired request must receive terminal");
    assert!(
        matches!(terminal, StreamTerminal::Error(ref msg) if msg.contains("deadline")),
        "expired stream must not report successful completion"
    );
    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(1), worker).await;
}
