//! #219: the parent Tokio runtime may shut down while its native inference
//! remains admitted. Both actual worker entrypoints must finish reactor-backed
//! model futures on the worker-owned runtime after parent shutdown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::engine::{
    EmbeddingResult, FinishReason, GenerationResult, InferenceCapability, InferenceConfig,
    InferenceEngine, InferenceError, InferenceInput, InferenceOutput, InferenceParams, Model,
};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::ModelHandle;
use crate::scheduler::queue::{QueuedRequest, TypedQueuedRequest};

#[derive(Clone, Copy)]
enum Mode {
    Generation,
    Embedding,
}

struct TimedModel {
    mode: Mode,
    started: mpsc::SyncSender<()>,
    release: Arc<Notify>,
    finished: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl Model for TimedModel {
    fn model_id(&self) -> &str {
        "reactor-backed"
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[
            InferenceCapability::TextGeneration,
            InferenceCapability::Embedding,
        ]
    }

    fn memory_usage(&self) -> usize {
        0
    }

    fn working_memory_estimate(&self) -> Option<usize> {
        Some(64)
    }

    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        self.started.send(()).expect("native inference started");
        // Hold until *after* the parent reactor has shut down, then await a
        // native-owned Tokio timer. A stale Handle::block_on will not wake.
        self.release.notified().await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.finished.store(true, Ordering::Release);
        Ok(match self.mode {
            Mode::Generation => InferenceOutput::Generation(GenerationResult {
                text: "complete".into(),
                tokens_generated: 1,
                finish_reason: FinishReason::Stop,
            }),
            Mode::Embedding => InferenceOutput::Embedding(EmbeddingResult {
                vector: vec![0.5],
                dimensions: 1,
            }),
        })
    }

    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn parent_shutdown_does_not_strand_native(mode: Mode) {
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let release = Arc::new(Notify::new());
    let finished = Arc::new(AtomicBool::new(false));
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 64,
        max_total_memory: 64,
        max_concurrent: 1,
    });

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("parent runtime");
    runtime.block_on(async {
        let engine = Arc::new(InferenceEngine::new(4096));
        engine
            .register_model(
                "reactor-backed".into(),
                ModelHandle::new(1),
                Arc::new(TimedModel {
                    mode,
                    started: started_tx,
                    release: Arc::clone(&release),
                    finished: Arc::clone(&finished),
                }),
            )
            .await;
        let engine = Arc::clone(&engine);
        let limits = limits.clone();
        tokio::spawn(async move {
            match mode {
                Mode::Generation => {
                    let request = QueuedRequest::new(
                        1,
                        "reactor-backed".into(),
                        "hello".into(),
                        InferenceParams::default(),
                    );
                    super::super::execute_request(
                        &engine,
                        None,
                        None,
                        Some(&limits),
                        None,
                        request,
                    )
                    .await;
                }
                Mode::Embedding => {
                    let request = TypedQueuedRequest {
                        id: 1,
                        model_id: "reactor-backed".into(),
                        input: InferenceInput::Text("hello".into()),
                        capability: InferenceCapability::Embedding,
                        params: InferenceParams::default(),
                        deadline: None,
                        cancelled: Arc::new(AtomicBool::new(false)),
                        response_tx: None,
                    };
                    crate::scheduler::worker_typed::execute(&engine, Some(&limits), None, request)
                        .await;
                }
            }
        });
    });
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("native inference never began");
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (64, 1)
    );

    // Async caller is gone, but a native admitted task must keep executing
    // independently and hold its reservation until the model actually ends.
    runtime.shutdown_timeout(Duration::from_millis(30));
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (64, 1)
    );
    assert!(!finished.load(Ordering::Acquire));
    release.notify_one();

    let start = Instant::now();
    while !finished.load(Ordering::Acquire) || limits.current_concurrent() != 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "native work remained stuck on the shut-down parent reactor"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (0, 0)
    );
}

#[test]
fn generation_native_timer_completes_after_parent_runtime_shutdown() {
    parent_shutdown_does_not_strand_native(Mode::Generation);
}

#[test]
fn typed_native_timer_completes_after_parent_runtime_shutdown() {
    parent_shutdown_does_not_strand_native(Mode::Embedding);
}
