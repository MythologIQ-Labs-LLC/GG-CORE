//! A stream's blocking task must own what it uses (#137 dump, 0xc0000005).
//!
//! `spawn_blocking` work is not cancelled when the awaiting future is
//! dropped. When the worker's streaming future was dropped mid-stream, the
//! blocking task kept running on borrowed engine and pipeline pointers and
//! faulted in the terminal sanitizer flush after the owners were freed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engine::gguf::{test_fixture, GgufConfig, GgufGenerator};
use crate::engine::{InferenceConfig, InferenceEngine, StreamItem, TokenStream};
use crate::models::ModelHandle;
use crate::scheduler::streaming_queue::StreamingQueuedRequest;
use crate::scheduler::worker_streaming;
use crate::security::{SecurityConfig, SecurityPipeline};

fn redacting_pipeline() -> Arc<SecurityPipeline> {
    Arc::new(SecurityPipeline::from_config(&SecurityConfig {
        enable_prompt_injection_detection: true,
        block_prompt_injection: true,
        enable_pii_detection: true,
        redact_pii: true,
        enable_model_encryption: false,
        encryption_key: None,
    }))
}

async fn engine_with_model(path: &std::path::Path) -> Arc<InferenceEngine> {
    let gguf = GgufConfig {
        n_ctx: 1024,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    let model = GgufGenerator::load("qwen".into(), path, &gguf).expect("load");
    let engine = Arc::new(InferenceEngine::new(4096));
    engine
        .register_model("qwen".into(), ModelHandle::new(1), Arc::new(model))
        .await;
    engine
}

fn long_request(token_sender: crate::engine::TokenStreamSender) -> StreamingQueuedRequest {
    StreamingQueuedRequest {
        id: 1,
        model_id: "qwen".into(),
        prompt: "Write the numbers from 1 to 400, one per line.".into(),
        config: InferenceConfig {
            max_tokens: Some(200),
            ..InferenceConfig::default()
        },
        enqueued_at: Instant::now(),
        deadline: None,
        cancelled: tokio_util::sync::CancellationToken::new(),
        token_sender,
    }
}

/// Wait until only the test holds `owned`, or fail after `limit`.
async fn released<T>(owned: &Arc<T>, limit: Duration) {
    let start = Instant::now();
    while Arc::strong_count(owned) > 1 {
        assert!(start.elapsed() < limit, "stream never released its clone");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn drop_caller_mid_stream(path: &std::path::Path) {
    let engine = engine_with_model(path).await;
    let pipeline = redacting_pipeline();
    let (tx, mut rx) = TokenStream::new(4, crate::engine::StreamBound::default());
    let (e, p) = (Arc::clone(&engine), Arc::clone(&pipeline));
    let caller = tokio::spawn(async move {
        worker_streaming::execute(&e, None, Some(&p), long_request(tx)).await;
    });
    let first = tokio::time::timeout(Duration::from_secs(120), rx.next()).await;
    assert!(first.expect("first frame deadline").is_some(), "no frame");
    caller.abort();
    assert!(caller.await.expect_err("aborted").is_cancelled());
    // The dropped caller's borrows are gone; the running stream must still
    // own the engine and pipeline it is using.
    assert!(
        Arc::strong_count(&pipeline) > 1,
        "stream borrowed the pipeline"
    );
    assert!(Arc::strong_count(&engine) > 1, "stream borrowed the engine");
    let drain = async { while !matches!(rx.next().await, None | Some(StreamItem::End(_))) {} };
    tokio::time::timeout(Duration::from_secs(300), drain)
        .await
        .expect("stream end deadline");
    released(&pipeline, Duration::from_secs(30)).await;
    released(&engine, Duration::from_secs(30)).await;
}

#[test]
fn dropped_caller_leaves_the_stream_owning_engine_and_pipeline() {
    let Some(path) = test_fixture::real_model() else {
        return;
    };
    let _serial = test_fixture::serial();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(drop_caller_mid_stream(path));
}
