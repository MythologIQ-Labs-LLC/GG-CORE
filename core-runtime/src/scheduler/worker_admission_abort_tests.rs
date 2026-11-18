//! Model-free integration regression for the *actual* worker entrypoints.
//! The backend deliberately blocks while its async worker task is aborted.
//! Both text-generation and typed paths must keep charged admission until
//! their native blocking closures finish (#219). No real GGUF/ONNX claim.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

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

struct Gate {
    started: Notify,
    release: Notify,
}

struct BlockedModel {
    mode: Mode,
    gate: Arc<Gate>,
}

#[async_trait::async_trait]
impl Model for BlockedModel {
    fn model_id(&self) -> &str {
        "held"
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration, InferenceCapability::Embedding]
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
        self.gate.started.notify_one();
        tokio::time::timeout(Duration::from_secs(10), self.gate.release.notified())
            .await
            .expect("test backend was never released");
        Ok(match self.mode {
            Mode::Generation => InferenceOutput::Generation(GenerationResult {
                text: "ok".into(),
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

async fn run_aborted_entrypoint(mode: Mode) {
    let gate = Arc::new(Gate {
        started: Notify::new(),
        release: Notify::new(),
    });
    let engine = Arc::new(InferenceEngine::new(4096));
    engine
        .register_model(
            "held".into(),
            ModelHandle::new(1),
            Arc::new(BlockedModel {
                mode,
                gate: Arc::clone(&gate),
            }),
        )
        .await;
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 64,
        max_total_memory: 64,
        max_concurrent: 1,
    });
    let e = Arc::clone(&engine);
    let admitted = limits.clone();
    let caller = tokio::spawn(async move {
        match mode {
            Mode::Generation => {
                let request = QueuedRequest::new(
                    1,
                    "held".into(),
                    "hello".into(),
                    InferenceParams::default(),
                );
                super::execute_request(&e, None, None, Some(&admitted), None, request).await;
            }
            Mode::Embedding => {
                let request = TypedQueuedRequest {
                    id: 1,
                    model_id: "held".into(),
                    input: InferenceInput::Text("hello".into()),
                    capability: InferenceCapability::Embedding,
                    params: InferenceParams::default(),
                    deadline: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                    response_tx: None,
                };
                crate::scheduler::worker_typed::execute(
                    &e,
                    Some(&admitted),
                    None,
                    request,
                )
                .await;
            }
        }
    });

    tokio::time::timeout(Duration::from_secs(5), gate.started.notified())
        .await
        .expect("native blocking work did not begin");
    caller.abort();
    assert!(caller.await.expect_err("caller aborted").is_cancelled());

    assert_eq!((limits.current_memory(), limits.current_concurrent()), (64, 1));
    assert!(
        limits.try_acquire_working(1).is_err(),
        "a freed waiter admitted another request while native work was active"
    );
    gate.release.notify_one();

    tokio::time::timeout(Duration::from_secs(5), async {
        while limits.current_concurrent() != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native completion failed to return the slot");
    assert_eq!((limits.current_memory(), limits.current_concurrent()), (0, 0));
    assert!(limits.try_acquire_working(64).is_ok(), "slot was not reusable");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_generation_worker_keeps_admission_until_backend_finishes() {
    run_aborted_entrypoint(Mode::Generation).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_typed_worker_keeps_admission_until_backend_finishes() {
    run_aborted_entrypoint(Mode::Embedding).await;
}
