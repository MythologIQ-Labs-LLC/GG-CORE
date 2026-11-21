//! P0 #219: a refused native model must be destroyed outside the lifecycle
//! write lock. A paused real destructor models a slow GGUF teardown; a
//! completely unrelated registration must still complete before it exits.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig, InferenceError,
    InferenceInput, InferenceOutput, Model,
};

struct HeldDestructor {
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    finished: Arc<AtomicBool>,
    size: usize,
}

impl Drop for HeldDestructor {
    fn drop(&mut self) {
        self.entered.send(()).expect("signal destructor entry");
        self.release
            .lock()
            .expect("release gate lock")
            .recv_timeout(Duration::from_secs(10))
            .expect("test must release the native destructor");
        self.finished.store(true, Ordering::Release);
    }
}

#[async_trait::async_trait]
impl Model for HeldDestructor {
    fn model_id(&self) -> &str {
        "loser"
    }

    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }

    fn memory_usage(&self) -> usize {
        self.size
    }

    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: String::new(),
            tokens_generated: 0,
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

async fn refusal_does_not_hold_writer(duplicate: bool) {
    use super::resident_tests::{meta, setup, Weights};

    let (lifecycle, limits) = setup(1_000);
    let lifecycle = Arc::new(lifecycle);
    lifecycle
        .load("winner".into(), meta("winner", 100), Arc::new(Weights(100)))
        .await
        .expect("initial registered model");

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let finished = Arc::new(AtomicBool::new(false));
    let model = Arc::new(HeldDestructor {
        entered: entered_tx,
        release: Mutex::new(release_rx),
        finished: Arc::clone(&finished),
        size: if duplicate { 25 } else { 1_200 },
    });
    let id = if duplicate { "winner" } else { "oversized" };
    let losing_lifecycle = Arc::clone(&lifecycle);
    let losing = tokio::spawn(async move {
        losing_lifecycle
            .load(id.into(), meta(id, model.size as u64), model)
            .await
    });

    // Wait for the actual refused model destructor to start, not a guessed
    // scheduling delay. The timeout guards a broken test, not admission.
    tokio::task::spawn_blocking(move || {
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("losing destructor was never entered");
    })
    .await
    .expect("gate observer");

    let unrelated = tokio::time::timeout(
        Duration::from_secs(2),
        lifecycle.load(
            "independent".into(),
            meta("independent", 100),
            Arc::new(Weights(100)),
        ),
    )
    .await;

    // Always release the blocked destructor, including when the regression
    // fails, so a red CI run cannot strand a blocking-pool thread.
    release_tx.send(()).expect("release native destructor");
    assert!(
        unrelated.is_ok(),
        "unrelated model registration waited on a refused model destructor"
    );
    unrelated
        .expect("unrelated load deadline")
        .expect("unrelated model fits");
    let failure = losing.await.expect("loser task join");
    if duplicate {
        assert!(matches!(failure, Err(LifecycleError::AlreadyLoaded(_))));
    } else {
        assert!(matches!(failure, Err(LifecycleError::ResourceExhausted(_))));
    }
    assert!(finished.load(Ordering::Acquire));
    assert_eq!(limits.current_memory(), 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn duplicate_id_refusal_destroys_backend_outside_index_writer() {
    refusal_does_not_hold_writer(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn budget_refusal_destroys_backend_outside_index_writer() {
    refusal_does_not_hold_writer(false).await;
}
