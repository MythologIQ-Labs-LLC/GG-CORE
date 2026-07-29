// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Chaos & Resilience - Scheduler and Inference Engine
//!
//! Queue flooding, concurrent enqueue/dequeue, expired request
//! handling, and inference engine edge cases.

use std::sync::Arc;
use std::time::Duration;

use gg_core::engine::InferenceParams;
use gg_core::scheduler::{Priority, RequestQueue, RequestQueueConfig};

// ============================================================================
// Queue Chaos
// ============================================================================

#[tokio::test]
async fn chaos_queue_flood() {
    let queue = RequestQueue::new(RequestQueueConfig {
        max_pending: 5,
        ..Default::default()
    });
    for i in 0..5 {
        let r = queue
            .enqueue(
                "model".into(),
                format!("prompt {}", i),
                InferenceParams::default(),
                Priority::Normal,
            )
            .await;
        assert!(r.is_ok(), "Request {} should enqueue", i);
    }
    let overflow = queue
        .enqueue(
            "model".into(),
            "overflow prompt".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await;
    assert!(overflow.is_err(), "Queue should reject when full");
}

#[tokio::test]
async fn chaos_queue_cancel_then_dequeue() {
    let queue = RequestQueue::new(RequestQueueConfig {
        max_pending: 10,
        ..Default::default()
    });
    let (id1, _) = queue
        .enqueue(
            "model".into(),
            "first prompt".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();
    let (id2, _) = queue
        .enqueue(
            "model".into(),
            "second prompt".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();
    assert!(queue.cancel(id1).await);
    let next = queue.dequeue().await.unwrap();
    assert_eq!(next.id, id2);
}

#[tokio::test]
async fn chaos_queue_expired_requests_skipped() {
    let queue = RequestQueue::new(RequestQueueConfig {
        max_pending: 10,
        ..Default::default()
    });
    let short = InferenceParams {
        timeout_ms: Some(1),
        ..Default::default()
    };
    queue
        .enqueue(
            "model".into(),
            "expiring prompt".into(),
            short,
            Priority::Normal,
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    queue
        .enqueue(
            "model".into(),
            "persistent prompt".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();
    let next = queue.dequeue().await.unwrap();
    assert_eq!(next.prompt, "persistent prompt");
}

#[tokio::test]
async fn chaos_concurrent_enqueue_dequeue() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig {
        max_pending: 256,
        ..Default::default()
    }));
    let mut handles = vec![];
    for pid in 0..4 {
        let q = Arc::clone(&queue);
        handles.push(tokio::spawn(async move {
            let mut n = 0u32;
            for i in 0..25 {
                if q.enqueue(
                    format!("model-{}", pid),
                    format!("prompt-{}-{}", pid, i),
                    InferenceParams::default(),
                    Priority::Normal,
                )
                .await
                .is_ok()
                {
                    n += 1;
                }
            }
            n
        }));
    }
    for _ in 0..2 {
        let q = Arc::clone(&queue);
        handles.push(tokio::spawn(async move {
            let mut n = 0u32;
            for _ in 0..50 {
                if q.dequeue().await.is_some() {
                    n += 1;
                }
                tokio::task::yield_now().await;
            }
            n
        }));
    }
    let results: Vec<u32> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert!(results[..4].iter().sum::<u32>() > 0);
}
