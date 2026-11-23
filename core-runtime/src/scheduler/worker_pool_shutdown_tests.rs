//! P0 #219: shutdown must never start queued native work after cancellation.
//! Model-free controls; real Windows GGUF shutdown still requires qualification.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;

use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig, InferenceEngine,
    InferenceError, InferenceInput, InferenceOutput, InferenceParams, Model, StreamBound,
    StreamTerminal, TokenStream,
};
use crate::models::ModelHandle;
use crate::scheduler::queue::{RequestQueue, RequestQueueConfig};
use crate::scheduler::{Priority, QueueError};

fn worker(
    queue: Arc<RequestQueue>,
    engine: Arc<InferenceEngine>,
    stop: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    super::spawn(
        queue,
        super::WorkerCtx {
            engine,
            lifecycle: None,
            registry: None,
            limits: None,
            security: None,
        },
        stop,
    )
}

#[tokio::test]
async fn pre_cancelled_shutdown_drains_all_three_queues_with_explicit_errors() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let (_, regular) = queue
        .enqueue_with_response(
            "absent".into(),
            "hello".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();
    let (stream_tx, stream_rx) = TokenStream::new(8, StreamBound::default());
    queue
        .enqueue_streaming(
            "absent".into(),
            "hello".into(),
            InferenceConfig::default(),
            stream_tx,
        )
        .await
        .unwrap();
    let (_, typed) = queue
        .enqueue_typed_controlled(
            "absent".into(),
            InferenceInput::Text("hello".into()),
            InferenceCapability::Embedding,
            InferenceParams::default(),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();

    // No worker may begin inference after shutdown was signalled, even if
    // every queue already contains work. Every caller gets a terminal.
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    let handle = worker(
        Arc::clone(&queue),
        Arc::new(InferenceEngine::new(4096)),
        shutdown,
    );
    tokio::time::timeout(Duration::from_secs(3), handle)
        .await
        .expect("shutdown must not process queued inference")
        .expect("worker pool joined");

    let main = regular.await.expect("main response channel");
    assert!(main.unwrap_err().to_lowercase().contains("cancel"));
    let typed = typed.await.expect("typed response channel");
    assert!(typed.unwrap_err().to_lowercase().contains("cancel"));
    let (tokens, terminal) = stream_rx.collect().await;
    assert!(tokens.is_empty());
    assert!(
        matches!(terminal, StreamTerminal::Error(ref text) if text.contains("cancelled")),
        "queued stream must receive an explicit shutdown terminal"
    );
    assert_eq!(queue.len().await, 0);
    assert_eq!(queue.streaming_len().await, 0);
    assert_eq!(queue.typed_len().await, 0);
    assert!(queue.is_closed(), "worker shutdown must seal its intake");
    assert!(matches!(
        queue
            .enqueue("absent".into(), "late".into(), InferenceParams::default(), Priority::Normal)
            .await,
        Err(QueueError::ShuttingDown)
    ));
    let (tx, _rx) = TokenStream::new(4, StreamBound::default());
    assert!(matches!(
        queue
            .enqueue_streaming("absent".into(), "late".into(), InferenceConfig::default(), tx)
            .await,
        Err(QueueError::ShuttingDown)
    ));
    assert!(matches!(
        queue
            .enqueue_typed_controlled(
                "absent".into(),
                InferenceInput::Text("late".into()),
                InferenceCapability::Embedding,
                InferenceParams::default(),
                Arc::new(AtomicBool::new(false)),
            )
            .await,
        Err(QueueError::ShuttingDown)
    ));
}

struct HeldModel {
    started: Mutex<Option<oneshot::Sender<()>>>,
    release: Arc<Notify>,
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Model for HeldModel {
    fn model_id(&self) -> &str {
        "held"
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }

    fn memory_usage(&self) -> usize {
        1024
    }

    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(tx) = self.started.lock().unwrap().take() {
            let _ = tx.send(());
        }
        self.release.notified().await;
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "ok".into(),
            tokens_generated: 1,
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_finishes_active_request_but_refuses_queued_work() {
    let queue = Arc::new(RequestQueue::new(RequestQueueConfig::default()));
    let engine = Arc::new(InferenceEngine::new(4096));
    let (started_tx, started_rx) = oneshot::channel();
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    engine
        .register_model(
            "held".into(),
            ModelHandle::new(1),
            Arc::new(HeldModel {
                started: Mutex::new(Some(started_tx)),
                release: Arc::clone(&release),
                calls: Arc::clone(&calls),
            }),
        )
        .await;
    let (_, current) = queue
        .enqueue_with_response(
            "held".into(),
            "a".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();
    let shutdown = CancellationToken::new();
    let handle = worker(Arc::clone(&queue), engine, shutdown.clone());
    tokio::time::timeout(Duration::from_secs(3), started_rx)
        .await
        .expect("first request began")
        .expect("first request sent start signal");

    let (_, pending) = queue
        .enqueue_with_response(
            "held".into(),
            "b".into(),
            InferenceParams::default(),
            Priority::Normal,
        )
        .await
        .unwrap();
    shutdown.cancel();
    // Supervisor must seal intake *while* the active model call is still
    // held; it must not wait for the worker's native call to finish.
    tokio::time::timeout(Duration::from_secs(3), async {
        while !queue.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("queue did not close during active inference");
    assert!(matches!(
        queue
            .enqueue("held".into(), "late".into(), InferenceParams::default(), Priority::Normal)
            .await,
        Err(QueueError::ShuttingDown)
    ));
    assert!(
        !handle.is_finished(),
        "shutdown must not abandon active work"
    );
    release.notify_one();
    let first = current.await.expect("active request received response");
    assert!(first.is_ok(), "{first:?}");
    tokio::time::timeout(Duration::from_secs(3), handle)
        .await
        .expect("worker exited after active request")
        .expect("worker pool joined");
    let second = pending.await.expect("queued request received cancellation");
    assert!(second.unwrap_err().to_lowercase().contains("cancel"));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "queued request must never execute"
    );
}
