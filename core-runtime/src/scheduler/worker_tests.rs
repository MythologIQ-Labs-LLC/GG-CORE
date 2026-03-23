//! Tests for the worker dequeue-execute loop.

use std::sync::Arc;

use super::*;
use crate::engine::inference::InferenceEngine;
use crate::engine::gguf::GgufModel;
use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig,
    InferenceError as EngineError, InferenceInput, InferenceOutput, InferenceParams,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig};
use crate::scheduler::Priority;

struct MockModel {
    id: String,
}

impl MockModel {
    fn arc(id: &str) -> Arc<dyn GgufModel> {
        Arc::new(Self { id: id.to_string() })
    }
}

#[async_trait::async_trait]
impl GgufModel for MockModel {
    fn model_id(&self) -> &str { &self.id }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize { 1024 }
    async fn infer(
        &self, _: &InferenceInput, _: &InferenceConfig,
    ) -> Result<InferenceOutput, EngineError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "hello world".into(),
            tokens_generated: 2,
            finish_reason: FinishReason::Stop,
        }))
    }
    async fn unload(&mut self) -> Result<(), EngineError> { Ok(()) }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

async fn setup() -> (Arc<RequestQueue>, Arc<InferenceEngine>) {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig { max_pending: 64, ..Default::default() }));
    let engine = Arc::new(InferenceEngine::new(4096));
    let handle = crate::models::ModelHandle::new(1);
    engine.register_model("test-model".into(), handle, MockModel::arc("test")).await;
    (queue, engine)
}

#[tokio::test]
async fn worker_executes_enqueued_request() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();

    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());

    let (_id, rx) = queue
        .enqueue_with_response(
            "test-model".into(), "hi".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2), rx,
    ).await.unwrap().unwrap().unwrap();

    assert_eq!(result.output, "hello world");
    assert_eq!(result.tokens_generated, 2);

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn worker_skips_cancelled_request() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();

    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());

    // Enqueue and immediately cancel
    let (id, rx) = queue
        .enqueue_with_response(
            "test-model".into(), "cancel me".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();
    queue.cancel(id).await;

    // Enqueue a second request that should succeed
    let (_id2, rx2) = queue
        .enqueue_with_response(
            "test-model".into(), "keep me".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();

    // First request: cancelled (receiver dropped or error)
    let r1 = tokio::time::timeout(std::time::Duration::from_secs(2), rx).await;
    // Either the receiver gets an error or the channel is dropped
    assert!(r1.is_err() || r1.unwrap().is_err());

    // Second request: succeeds
    let r2 = tokio::time::timeout(std::time::Duration::from_secs(2), rx2)
        .await.unwrap().unwrap().unwrap();
    assert_eq!(r2.output, "hello world");

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn worker_handles_multiple_concurrent_requests() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();

    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());

    let mut receivers = Vec::new();
    for i in 0..5 {
        let (_id, rx) = queue
            .enqueue_with_response(
                "test-model".into(), format!("prompt {i}"),
                InferenceParams::default(), Priority::Normal,
            )
            .await.unwrap();
        receivers.push(rx);
    }

    for rx in receivers {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5), rx,
        ).await.unwrap().unwrap().unwrap();
        assert_eq!(result.output, "hello world");
    }

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn worker_shuts_down_gracefully() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();

    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());

    shutdown.cancel();
    queue.wake();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2), worker,
    ).await;
    assert!(result.is_ok(), "worker should shut down within timeout");
}

#[tokio::test]
async fn queue_full_rejected() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig { max_pending: 2, ..Default::default() }));

    queue.enqueue("m".into(), "a".into(), InferenceParams::default(), Priority::Normal)
        .await.unwrap();
    queue.enqueue("m".into(), "b".into(), InferenceParams::default(), Priority::Normal)
        .await.unwrap();

    let err = queue.enqueue("m".into(), "c".into(), InferenceParams::default(), Priority::Normal)
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn tier1_context_check_rejects_oversized_prompt() {
    // max_context_tokens=10 means max ~40 bytes before tier-1 rejects.
    let config = RequestQueueConfig {
        max_pending: 64,
        max_context_tokens: 10,
    };
    let queue = Arc::new(RequestQueue::new(config));

    // 44 bytes / 4 = 11 estimated tokens > 10 max => rejected
    let big_prompt = "a".repeat(44);
    let err = queue
        .enqueue("m".into(), big_prompt, InferenceParams::default(), Priority::Normal)
        .await;
    assert!(err.is_err(), "oversized prompt must be rejected at tier-1");

    // 40 bytes / 4 = 10 estimated tokens == 10 max => accepted
    let ok_prompt = "b".repeat(40);
    let result = queue
        .enqueue("m".into(), ok_prompt, InferenceParams::default(), Priority::Normal)
        .await;
    assert!(result.is_ok(), "prompt at limit should be accepted");
}

#[tokio::test]
async fn tier1_context_check_allows_small_prompt() {
    let config = RequestQueueConfig {
        max_pending: 64,
        max_context_tokens: 4096,
    };
    let queue = Arc::new(RequestQueue::new(config));

    let result = queue
        .enqueue("m".into(), "hello".into(), InferenceParams::default(), Priority::Normal)
        .await;
    assert!(result.is_ok());
}

// ---- Resource limits integration tests ----

/// Build a ResourceLimits with a very tight total memory cap (below MockModel's 1024 bytes)
/// but generous concurrency, so memory is the rejecting constraint.
fn limits_memory_exceeded() -> ResourceLimits {
    ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 512,      // per-call cap below 1024 bytes reported by MockModel
        max_total_memory: 1024 * 1024, // total is fine
        max_concurrent: 16,
    })
}

/// Build a ResourceLimits with a concurrency cap of 0 so every request is rejected.
fn limits_concurrency_exceeded() -> ResourceLimits {
    ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: usize::MAX,
        max_total_memory: usize::MAX,
        max_concurrent: 0, // no slots available
    })
}

#[tokio::test]
async fn worker_rejects_when_memory_limit_exceeded() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let limits = limits_memory_exceeded();

    let worker = spawn_worker_with_registry(
        queue.clone(),
        engine,
        None,
        None,
        Some(limits),
        shutdown.clone(),
    );

    let (_id, rx) = queue
        .enqueue_with_response(
            "test-model".into(), "hi".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2), rx,
    ).await.unwrap().unwrap();

    assert!(result.is_err(), "request should be rejected due to memory limit");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("memory") || msg.contains("Memory"),
        "error should mention memory, got: {msg}"
    );

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn worker_rejects_when_concurrency_limit_exceeded() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let limits = limits_concurrency_exceeded();

    let worker = spawn_worker_with_registry(
        queue.clone(),
        engine,
        None,
        None,
        Some(limits),
        shutdown.clone(),
    );

    let (_id, rx) = queue
        .enqueue_with_response(
            "test-model".into(), "hi".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2), rx,
    ).await.unwrap().unwrap();

    assert!(result.is_err(), "request should be rejected due to concurrency limit");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("queue") || msg.contains("Queue") || msg.contains("concurrent"),
        "error should mention queue/concurrency, got: {msg}"
    );

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn worker_releases_resource_guard_after_inference() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();

    // Allow exactly 1 concurrent request with enough memory.
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 2048,
        max_total_memory: 2 * 1024 * 1024,
        max_concurrent: 1,
    });

    let worker = spawn_worker_with_registry(
        queue.clone(),
        engine,
        None,
        None,
        Some(limits.clone()),
        shutdown.clone(),
    );

    // Send two sequential requests. If the guard is properly dropped after
    // the first, the second will be admitted.
    let (_id1, rx1) = queue
        .enqueue_with_response(
            "test-model".into(), "first".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();
    let r1 = tokio::time::timeout(
        std::time::Duration::from_secs(2), rx1,
    ).await.unwrap().unwrap();
    assert!(r1.is_ok(), "first request should succeed");

    // After first completes, concurrent count must be back to 0.
    assert_eq!(limits.current_concurrent(), 0, "guard should have been dropped");

    let (_id2, rx2) = queue
        .enqueue_with_response(
            "test-model".into(), "second".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();
    let r2 = tokio::time::timeout(
        std::time::Duration::from_secs(2), rx2,
    ).await.unwrap().unwrap();
    assert!(r2.is_ok(), "second request should succeed after guard is released");

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

#[tokio::test]
async fn admission_rejection_propagates_to_caller() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let limits = limits_concurrency_exceeded();

    let worker = spawn_worker_with_registry(
        queue.clone(),
        engine,
        None,
        None,
        Some(limits),
        shutdown.clone(),
    );

    let (_id, rx) = queue
        .enqueue_with_response(
            "test-model".into(), "probe".into(),
            InferenceParams::default(), Priority::Normal,
        )
        .await.unwrap();

    // The oneshot channel must deliver the Err — not drop or timeout.
    let channel_result = tokio::time::timeout(
        std::time::Duration::from_secs(2), rx,
    ).await;
    assert!(channel_result.is_ok(), "channel should not time out");

    let send_result = channel_result.unwrap();
    assert!(send_result.is_ok(), "oneshot should not be dropped");

    let inference_result = send_result.unwrap();
    assert!(inference_result.is_err(), "caller must receive the rejection error");

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

/// Streaming enqueue succeeds when queue has capacity.
#[tokio::test]
async fn streaming_enqueue_succeeds_with_capacity() {
    let config = RequestQueueConfig { max_pending: 10, max_context_tokens: 4096 };
    let queue = Arc::new(RequestQueue::new(config));

    let (tx, _rx) = crate::engine::TokenStream::new(4);
    let result = queue
        .enqueue_streaming("m".into(), "prompt".into(), InferenceConfig::default(), tx)
        .await;
    assert!(result.is_ok(), "streaming enqueue should succeed with capacity");
}

/// Streaming enqueue rejects when queue is full.
#[tokio::test]
async fn streaming_enqueue_rejects_when_queue_full() {
    let config = RequestQueueConfig { max_pending: 2, max_context_tokens: 4096 };
    let queue = Arc::new(RequestQueue::new(config));

    queue.enqueue("m".into(), "a".into(), InferenceParams::default(), Priority::Normal)
        .await.unwrap();
    queue.enqueue("m".into(), "b".into(), InferenceParams::default(), Priority::Normal)
        .await.unwrap();

    let (tx, _rx) = crate::engine::TokenStream::new(4);
    let result = queue
        .enqueue_streaming("m".into(), "c".into(), InferenceConfig::default(), tx)
        .await;
    assert!(result.is_err(), "streaming enqueue must fail when queue is full");
}

/// Resource limits rejection for streaming via worker.
#[tokio::test]
async fn worker_rejects_streaming_when_concurrency_limit_exceeded() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let limits = limits_concurrency_exceeded();

    let worker = spawn_worker_with_registry(
        queue.clone(), engine, None, None, Some(limits), shutdown.clone(),
    );

    let (tx, mut rx) = crate::engine::TokenStream::new(4);
    queue
        .enqueue_streaming(
            "test-model".into(), "hi".into(),
            InferenceConfig::default(), tx,
        )
        .await
        .unwrap();

    // Worker should send a final token to signal rejection.
    let output = tokio::time::timeout(std::time::Duration::from_secs(3), rx.next()).await;
    assert!(output.is_ok(), "should receive response from worker");
    if let Ok(Some(out)) = output {
        assert!(out.is_final, "rejection should send is_final=true");
    }

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}

/// Verifies that infer_cancellable delegates to infer by default.
#[tokio::test]
async fn default_infer_cancellable_delegates_to_infer() {
    let model = MockModel::arc("test");
    let input = InferenceInput::Text("hello".into());
    let config = InferenceConfig::default();
    let check = || false; // never cancelled

    let result = model.infer_cancellable(&input, &config, Some(&check)).await;
    assert!(result.is_ok());
    if let Ok(InferenceOutput::Generation(gen)) = result {
        assert_eq!(gen.text, "hello world");
    } else {
        panic!("expected generation output");
    }
}

/// Verifies that routing inference through enqueue_with_response (as Python
/// Session.infer() now does) delivers results from the worker, confirming
/// the queue-only execution invariant (C-1 fix).
#[tokio::test]
async fn python_session_infer_routes_through_queue() {
    let (queue, engine) = setup().await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = spawn_worker(queue.clone(), engine, shutdown.clone());

    // Simulate what Session::infer() does: enqueue_with_response + await rx
    let (_id, rx) = queue
        .enqueue_with_response(
            "test-model".into(),
            "python prompt".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert_eq!(result.output, "hello world");
    assert!(result.finished);

    shutdown.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
}
