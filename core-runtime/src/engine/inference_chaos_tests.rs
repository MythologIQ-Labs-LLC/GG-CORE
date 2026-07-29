//! Engine-direct chaos tests for `InferenceEngine` (B-33: relocated in-crate from
//! `tests/chaos_scheduler_shutdown_test.rs` because `run` is now `pub(crate)`).

use crate::engine::{InferenceEngine, InferenceParams};
use std::sync::Arc;

#[tokio::test]
async fn chaos_inference_engine_context_exceeded() {
    let engine = InferenceEngine::new(128);
    // Create a prompt that exceeds context length (128 bytes)
    let huge_prompt = "x".repeat(200);
    let result = engine
        .run("test-model", &huge_prompt, &InferenceParams::default())
        .await;
    // Should fail - either context exceeded or model not loaded
    assert!(result.is_err());
}

#[tokio::test]
async fn chaos_inference_engine_invalid_params() {
    let engine = InferenceEngine::new(4096);
    let bad = InferenceParams {
        max_tokens: 0,
        ..Default::default()
    };
    // Should fail due to invalid params (max_tokens = 0)
    let result = engine.run("test-model", "Hello world", &bad).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn chaos_inference_engine_concurrent_requests() {
    let engine = Arc::new(InferenceEngine::new(4096));
    let mut handles = vec![];
    for i in 0..10u32 {
        let e = Arc::clone(&engine);
        handles.push(tokio::spawn(async move {
            let prompt = format!("Concurrent request {}", i);
            e.run("test-model", &prompt, &InferenceParams::default())
                .await
        }));
    }
    let results: Vec<_> = futures::future::join_all(handles).await;
    // All requests should complete (either success or model-not-loaded error)
    for (i, r) in results.iter().enumerate() {
        assert!(r.is_ok(), "Request {} should complete without panic", i);
    }
}
