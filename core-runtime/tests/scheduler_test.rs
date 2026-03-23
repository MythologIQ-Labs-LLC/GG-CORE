//! TDD-Light tests for scheduler module.

use gg_core::engine::InferenceParams;
use gg_core::scheduler::{
    BatchConfig, BatchProcessor, Priority, PriorityQueue, RequestQueue,
    RequestQueueConfig, ThreadPoolConfig,
};

#[test]
fn priority_queue_orders_by_priority() {
    let mut queue: PriorityQueue<&str> = PriorityQueue::new();

    queue.push("low", Priority::Low);
    queue.push("critical", Priority::Critical);
    queue.push("normal", Priority::Normal);

    assert_eq!(queue.pop(), Some("critical"));
    assert_eq!(queue.pop(), Some("normal"));
    assert_eq!(queue.pop(), Some("low"));
}

#[test]
fn priority_queue_fifo_within_same_priority() {
    let mut queue: PriorityQueue<&str> = PriorityQueue::new();

    queue.push("first", Priority::Normal);
    queue.push("second", Priority::Normal);
    queue.push("third", Priority::Normal);

    assert_eq!(queue.pop(), Some("first"));
    assert_eq!(queue.pop(), Some("second"));
    assert_eq!(queue.pop(), Some("third"));
}

#[tokio::test]
async fn request_queue_enqueue_dequeue() {
    let queue = RequestQueue::new(RequestQueueConfig::default());

    let (id, position) = queue
        .enqueue(
            "model".to_string(),
            "test prompt".to_string(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();

    assert_eq!(id, 1);
    assert_eq!(position, 0);

    let request = queue.dequeue().await.unwrap();
    assert_eq!(request.id, 1);
    assert_eq!(request.model_id, "model");
}

#[test]
fn batch_processor_respects_size_limit() {
    let config = BatchConfig {
        max_batch_size: 2,
        max_total_tokens: 1000,
    };
    let processor = BatchProcessor::new(config);

    let requests = vec![
        create_test_request(1, 10),
        create_test_request(2, 10),
        create_test_request(3, 10),
    ];

    let batches = processor.create_batches(requests);

    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 2);
    assert_eq!(batches[1].len(), 1);
}

#[test]
fn batch_processor_respects_token_limit() {
    let config = BatchConfig {
        max_batch_size: 10,
        max_total_tokens: 25,
    };
    let processor = BatchProcessor::new(config);

    let requests = vec![
        create_test_request(1, 10),
        create_test_request(2, 10),
        create_test_request(3, 10),
    ];

    let batches = processor.create_batches(requests);

    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].total_tokens, 20);
    assert_eq!(batches[1].total_tokens, 10);
}

fn create_test_request(
    id: u64,
    token_count: usize,
) -> gg_core::scheduler::QueuedRequest {
    // Create prompt with ~4 chars per token (batch processor estimates tokens from bytes)
    let prompt = "x".repeat(token_count * 4);
    gg_core::scheduler::QueuedRequest::new(
        id,
        "test".to_string(),
        prompt,
        InferenceParams::default(),
    )
}

// Thread pool configuration tests

#[test]
fn thread_pool_config_default_uses_available() {
    let config = ThreadPoolConfig::default();

    // Should use available parallelism or fallback to 4
    let expected = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    assert_eq!(config.worker_threads.get(), expected);
}

#[test]
fn thread_pool_config_minimum_one_thread() {
    // Requesting 0 threads should give at least 1
    let config = ThreadPoolConfig::with_threads(0);

    assert_eq!(config.worker_threads.get(), 1);
}

#[test]
fn thread_pool_config_inference_halves_cores() {
    let config = ThreadPoolConfig::for_inference();

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // Should be half of cores, minimum 2
    let expected = (cores / 2).max(2);
    assert_eq!(config.worker_threads.get(), expected);
}

#[test]
fn thread_pool_config_stack_size_reasonable() {
    let default_config = ThreadPoolConfig::default();
    let inference_config = ThreadPoolConfig::for_inference();

    // Default stack should be at least 2MB
    assert!(default_config.stack_size >= 2 * 1024 * 1024);

    // Inference stack should be at least 4MB
    assert!(inference_config.stack_size >= 4 * 1024 * 1024);
}
