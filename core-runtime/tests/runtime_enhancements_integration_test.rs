//! Integration tests for runtime enhancements bundle.
//!
//! Tests the combined functionality of timeout/cancellation,
//! warmup, deduplication, and connection management.

use std::time::Duration;

use gg_core::engine::InferenceParams;
use gg_core::ipc::{
    decode_message, encode_message, ConnectionConfig, ConnectionPool,
    IpcMessage, WarmupRequest, WarmupResponse,
};
use gg_core::scheduler::{OutputCache, OutputCacheConfig, Priority, RequestQueue, RequestQueueConfig};

#[tokio::test]
async fn test_cancel_pending_request() {
    let queue = RequestQueue::new(RequestQueueConfig::default());
    let params = InferenceParams {
        max_tokens: 100,
        ..Default::default()
    };

    // Enqueue a request (text-based protocol)
    let (id, _pos) = queue
        .enqueue("model".to_string(), "test prompt".to_string(), params, Priority::Normal)
        .await
        .unwrap();

    // Cancel it
    let cancelled = queue.cancel(id).await;
    assert!(cancelled);

    // Dequeue should return None (cancelled)
    let result = queue.dequeue().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_timeout_during_queue() {
    let queue = RequestQueue::new(RequestQueueConfig::default());

    // Request with very short timeout
    let params = InferenceParams {
        timeout_ms: Some(1),
        ..Default::default()
    };

    queue
        .enqueue("model".to_string(), "test prompt".to_string(), params, Priority::Normal)
        .await
        .unwrap();

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Should skip expired request
    let result = queue.dequeue().await;
    assert!(result.is_none());
}

#[test]
fn test_warmup_then_inference() {
    // Test warmup message encoding
    let warmup_req = IpcMessage::WarmupRequest(WarmupRequest {
        model_id: "test-model".to_string(),
        tokens: 1,
    });

    let bytes = encode_message(&warmup_req).unwrap();
    let decoded = decode_message(&bytes).unwrap();

    match decoded {
        IpcMessage::WarmupRequest(req) => {
            assert_eq!(req.model_id, "test-model");
            assert_eq!(req.tokens, 1);
        }
        _ => panic!("Expected WarmupRequest"),
    }

    // Test warmup response
    let warmup_resp = IpcMessage::WarmupResponse(WarmupResponse {
        model_id: "test-model".to_string(),
        success: true,
        error: None,
        elapsed_ms: 42,
    });

    let bytes = encode_message(&warmup_resp).unwrap();
    let decoded = decode_message(&bytes).unwrap();

    match decoded {
        IpcMessage::WarmupResponse(resp) => {
            assert!(resp.success);
            assert!(resp.error.is_none());
        }
        _ => panic!("Expected WarmupResponse"),
    }
}

#[test]
fn test_dedup_across_sessions() {
    let config = OutputCacheConfig {
        ttl: Duration::from_secs(60),
        max_entries: 100,
    };
    let mut cache = OutputCache::new(config);

    let params = InferenceParams::default();
    let tokens = vec![1, 2, 3, 4, 5];

    // First request - cache miss
    let key = OutputCache::cache_key(&tokens, &params);
    let cached = cache.get(&key);
    assert!(cached.is_none());

    // Store result
    cache.insert(key, vec![10, 20, 30]);

    // Second request with same prompt - cache hit
    let key2 = OutputCache::cache_key(&tokens, &params);
    let cached = cache.get(&key2);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().output_tokens, vec![10, 20, 30]);
}

#[test]
fn test_connection_limit_enforced() {
    let config = ConnectionConfig { max_connections: 2 };
    let pool = ConnectionPool::new(config);

    // Acquire up to limit
    let _guard1 = pool.try_acquire().expect("should succeed");
    let _guard2 = pool.try_acquire().expect("should succeed");
    assert_eq!(pool.active_count(), 2);

    // Should fail - at limit
    let guard3 = pool.try_acquire();
    assert!(guard3.is_none());
    assert_eq!(pool.active_count(), 2);

    // Drop one, should be able to acquire again
    drop(_guard1);
    assert_eq!(pool.active_count(), 1);

    let guard4 = pool.try_acquire();
    assert!(guard4.is_some());
    assert_eq!(pool.active_count(), 2);
}
