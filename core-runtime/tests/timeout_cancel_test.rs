//! Tests for request timeout and cancellation functionality.

use std::time::{Duration, Instant};

use gg_core::engine::InferenceParams;
use gg_core::ipc::{decode_message, encode_message, IpcMessage, RequestId};
use gg_core::scheduler::{Priority, RequestQueue, RequestQueueConfig};

#[test]
fn test_request_with_timeout() {
    let params = InferenceParams {
        timeout_ms: Some(1000),
        ..Default::default()
    };
    assert_eq!(params.timeout_ms, Some(1000));
}

#[test]
fn test_request_no_timeout() {
    let params = InferenceParams::default();
    assert_eq!(params.timeout_ms, None);
}

#[tokio::test]
async fn test_request_deadline_computed() {
    let queue = RequestQueue::new(RequestQueueConfig::default());
    let params = InferenceParams {
        timeout_ms: Some(5000), // 5 second timeout
        ..Default::default()
    };

    let before = Instant::now();
    let (id, _pos) = queue
        .enqueue("model".to_string(), "test prompt".to_string(), params, Priority::Normal)
        .await
        .unwrap();

    // Dequeue to inspect the request
    let request = queue.dequeue().await.unwrap();
    assert_eq!(request.id, id);
    assert!(request.deadline.is_some());

    // Deadline should be approximately 5 seconds from enqueue time
    let deadline = request.deadline.unwrap();
    let expected = before + Duration::from_millis(5000);
    assert!(deadline >= expected - Duration::from_millis(100));
    assert!(deadline <= expected + Duration::from_millis(100));
}

#[tokio::test]
async fn test_request_no_deadline_when_no_timeout() {
    let queue = RequestQueue::new(RequestQueueConfig::default());
    let params = InferenceParams::default(); // No timeout

    queue
        .enqueue("model".to_string(), "test prompt".to_string(), params, Priority::Normal)
        .await
        .unwrap();

    let request = queue.dequeue().await.unwrap();
    assert!(request.deadline.is_none());
}

#[tokio::test]
async fn test_cancel_pending_request() {
    let queue = RequestQueue::new(RequestQueueConfig::default());
    let params = InferenceParams::default();

    let (id, _) = queue
        .enqueue("model".to_string(), "test prompt".to_string(), params, Priority::Normal)
        .await
        .unwrap();

    // Cancel should succeed
    let cancelled = queue.cancel(id).await;
    assert!(cancelled);

    // Dequeue should return None (cancelled request skipped)
    let result = queue.dequeue().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cancel_unknown_request() {
    let queue = RequestQueue::new(RequestQueueConfig::default());

    // Cancel non-existent request should return false
    let cancelled = queue.cancel(999).await;
    assert!(!cancelled);
}

#[tokio::test]
async fn test_cancelled_request_skipped_on_dequeue() {
    let queue = RequestQueue::new(RequestQueueConfig::default());
    let params = InferenceParams::default();

    // Enqueue two requests
    let (id1, _) = queue
        .enqueue("model".to_string(), "first prompt".to_string(), params.clone(), Priority::Normal)
        .await
        .unwrap();
    let (id2, _) = queue
        .enqueue("model".to_string(), "second prompt".to_string(), params, Priority::Normal)
        .await
        .unwrap();

    // Cancel first request
    queue.cancel(id1).await;

    // Dequeue should skip cancelled and return second
    let request = queue.dequeue().await.unwrap();
    assert_eq!(request.id, id2);
}

#[test]
fn test_cancel_message_roundtrip() {
    let request_id = RequestId(42);
    let msg = IpcMessage::CancelRequest { request_id };

    let bytes = encode_message(&msg).unwrap();
    let decoded = decode_message(&bytes).unwrap();

    match decoded {
        IpcMessage::CancelRequest { request_id: rid } => {
            assert_eq!(rid.0, 42);
        }
        _ => panic!("Expected CancelRequest"),
    }
}

#[test]
fn test_cancel_response_roundtrip() {
    let request_id = RequestId(42);
    let msg = IpcMessage::CancelResponse {
        request_id,
        cancelled: true,
    };

    let bytes = encode_message(&msg).unwrap();
    let decoded = decode_message(&bytes).unwrap();

    match decoded {
        IpcMessage::CancelResponse {
            request_id: rid,
            cancelled,
        } => {
            assert_eq!(rid.0, 42);
            assert!(cancelled);
        }
        _ => panic!("Expected CancelResponse"),
    }
}

#[tokio::test]
async fn test_expired_request_skipped_on_dequeue() {
    let queue = RequestQueue::new(RequestQueueConfig::default());

    // Create request with very short timeout (1ms)
    let params = InferenceParams {
        timeout_ms: Some(1),
        ..Default::default()
    };

    queue
        .enqueue("model".to_string(), "expiring prompt".to_string(), params.clone(), Priority::Normal)
        .await
        .unwrap();

    // Wait for it to expire
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Enqueue a second request without timeout
    let params2 = InferenceParams::default();
    let (id2, _) = queue
        .enqueue("model".to_string(), "persistent prompt".to_string(), params2, Priority::Normal)
        .await
        .unwrap();

    // Dequeue should skip expired and return second
    let request = queue.dequeue().await.unwrap();
    assert_eq!(request.id, id2);
}
