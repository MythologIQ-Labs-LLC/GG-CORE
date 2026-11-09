//! Tests for request queue and streaming enqueue.

#[cfg(test)]
mod tests {
    use crate::engine::TokenStream;
    use crate::engine::{InferenceConfig, InferenceParams};
    use crate::scheduler::{Priority, RequestQueue, RequestQueueConfig};

    fn small_queue() -> RequestQueue {
        RequestQueue::new(RequestQueueConfig {
            max_pending: 2,
            max_context_tokens: 4096,
        })
    }

    #[tokio::test]
    async fn controlled_stream_carries_cancel_and_deadline() {
        let q = small_queue();
        let (tx, _rx) = TokenStream::new(4, crate::engine::StreamBound::default());
        let cancel = tokio_util::sync::CancellationToken::new();
        q.enqueue_streaming_controlled(
            "m".into(),
            "prompt".into(),
            InferenceConfig::default(),
            tx,
            cancel.clone(),
        )
        .await
        .unwrap();
        let request = q.dequeue_streaming().await.unwrap();
        assert!(request.deadline.is_some());
        assert!(!request.is_cancelled());
        cancel.cancel();
        assert!(request.is_cancelled());
    }

    #[tokio::test]
    async fn expired_stream_is_skipped() {
        let q = small_queue();
        let (tx, _rx) = TokenStream::new(4, crate::engine::StreamBound::default());
        let config = InferenceConfig {
            timeout_ms: 1,
            ..Default::default()
        };
        q.enqueue_streaming("m".into(), "p".into(), config, tx)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let expired = q
            .dequeue_streaming()
            .await
            .expect("worker must see expired work");
        assert!(expired.is_expired());
    }

    #[tokio::test]
    async fn enqueue_with_response_returns_receiver() {
        let q = small_queue();
        let result = q
            .enqueue_with_response(
                "model".into(),
                "hello".into(),
                InferenceParams::default(),
                Priority::Normal,
            )
            .await;
        assert!(result.is_ok());
        let (id, _rx) = result.unwrap();
        assert!(id > 0);
    }

    #[tokio::test]
    async fn queue_full_rejects_enqueue() {
        let q = small_queue();
        for _ in 0..2 {
            q.enqueue(
                "m".into(),
                "p".into(),
                InferenceParams::default(),
                Priority::Normal,
            )
            .await
            .unwrap();
        }
        let err = q
            .enqueue(
                "m".into(),
                "p".into(),
                InferenceParams::default(),
                Priority::Normal,
            )
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn streaming_enqueue_counts_toward_capacity() {
        let q = small_queue(); // max_pending = 2

        // Enqueue 2 streaming requests => queue should be full
        let (tx1, _rx1) = TokenStream::new(4, crate::engine::StreamBound::default());
        let (tx2, _rx2) = TokenStream::new(4, crate::engine::StreamBound::default());
        q.enqueue_streaming("m".into(), "a".into(), InferenceConfig::default(), tx1)
            .await
            .unwrap();
        q.enqueue_streaming("m".into(), "b".into(), InferenceConfig::default(), tx2)
            .await
            .unwrap();

        // Regular enqueue should fail
        let err = q
            .enqueue(
                "m".into(),
                "p".into(),
                InferenceParams::default(),
                Priority::Normal,
            )
            .await;
        assert!(err.is_err());

        // Third streaming should also fail
        let (tx3, _rx3) = TokenStream::new(4, crate::engine::StreamBound::default());
        let err = q
            .enqueue_streaming("m".into(), "c".into(), InferenceConfig::default(), tx3)
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn streaming_dequeue_returns_request() {
        let q = small_queue();
        let (tx, _rx) = TokenStream::new(4, crate::engine::StreamBound::default());
        let id = q
            .enqueue_streaming(
                "model".into(),
                "hello".into(),
                InferenceConfig::default(),
                tx,
            )
            .await
            .unwrap();

        let req = q.dequeue_streaming().await;
        assert!(req.is_some());
        assert_eq!(req.unwrap().id, id);
    }

    #[tokio::test]
    async fn dequeued_streaming_frees_capacity() {
        let q = small_queue(); // max_pending = 2

        let (tx1, _rx1) = TokenStream::new(4, crate::engine::StreamBound::default());
        let (tx2, _rx2) = TokenStream::new(4, crate::engine::StreamBound::default());
        q.enqueue_streaming("m".into(), "a".into(), InferenceConfig::default(), tx1)
            .await
            .unwrap();
        q.enqueue_streaming("m".into(), "b".into(), InferenceConfig::default(), tx2)
            .await
            .unwrap();

        // Dequeue one streaming request
        let _req = q.dequeue_streaming().await.unwrap();

        // Now there's space for one more
        let (tx3, _rx3) = TokenStream::new(4, crate::engine::StreamBound::default());
        let ok = q
            .enqueue_streaming("m".into(), "c".into(), InferenceConfig::default(), tx3)
            .await;
        assert!(ok.is_ok());
    }

    #[tokio::test]
    async fn context_too_large_rejects_streaming() {
        let q = RequestQueue::new(RequestQueueConfig {
            max_pending: 10,
            max_context_tokens: 10,
        });
        // 44 bytes / 4 = 11 > 10 max
        let big = "a".repeat(44);
        let (tx, _rx) = TokenStream::new(4, crate::engine::StreamBound::default());
        let err = q
            .enqueue_streaming("m".into(), big, InferenceConfig::default(), tx)
            .await;
        assert!(err.is_err());
    }

    /// Regression (RC acceptance): a streaming enqueue must wake a worker
    /// parked in `wait_and_dequeue` — it returns `None` so the worker loop
    /// can service the streaming queue, instead of re-awaiting forever.
    #[tokio::test]
    async fn streaming_enqueue_unparks_wait_and_dequeue() {
        let q = std::sync::Arc::new(RequestQueue::new(RequestQueueConfig {
            max_pending: 10,
            max_context_tokens: 4096,
        }));
        let waiter = {
            let q = q.clone();
            tokio::spawn(async move { q.wait_and_dequeue().await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await; // park it
        let (tx, _rx) = TokenStream::new(4, crate::engine::StreamBound::default());
        q.enqueue_streaming("m".into(), "a".into(), InferenceConfig::default(), tx)
            .await
            .unwrap();
        let woke = tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("wait_and_dequeue must return once streaming work exists")
            .unwrap();
        assert!(woke.is_none(), "no main-queue request was enqueued");
        assert_eq!(
            q.streaming_len().await,
            1,
            "streaming request still pending for the worker"
        );
    }
}
