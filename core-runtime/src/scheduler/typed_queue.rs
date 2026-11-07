//! Typed (embedding/classification) lane of the shared request queue.
//!
//! Child module of `queue` so it shares the queue's private state: one
//! `max_pending` bound across the generation, streaming and typed lanes,
//! one `Notify` for the single worker, and one lock order
//! (main -> streaming -> typed) everywhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{QueueError, RequestQueue};
use crate::engine::inference::InferenceError;
use crate::engine::StopControl;
use crate::engine::{InferenceCapability, InferenceInput, InferenceOutput, InferenceParams};

/// Worker -> handler result for one typed request.
pub type TypedResponseTx = tokio::sync::oneshot::Sender<Result<InferenceOutput, String>>;
pub type TypedResponseRx = tokio::sync::oneshot::Receiver<Result<InferenceOutput, String>>;

/// A queued typed request. Content is never logged (no Debug of input).
pub struct TypedQueuedRequest {
    pub id: u64,
    pub model_id: String,
    pub input: InferenceInput,
    pub capability: InferenceCapability,
    pub params: InferenceParams,
    pub deadline: Option<Instant>,
    pub cancelled: Arc<AtomicBool>,
    pub response_tx: Option<TypedResponseTx>,
}

impl TypedQueuedRequest {
    /// Cancel flag plus deadline, observed by the engine during execution.
    pub fn stop_control(&self) -> StopControl {
        StopControl::new(
            Some(Arc::clone(&self.cancelled)),
            self.deadline,
            self.params.timeout_ms.unwrap_or(0),
        )
    }

    /// Deliver the result (or error) to the waiting handler.
    pub fn respond(mut self, result: Result<InferenceOutput, String>) {
        if let Some(tx) = self.response_tx.take() {
            let _ = tx.send(result);
        }
    }

    /// Stopped before execution: explicit failure, no work done.
    pub fn reject_unexecuted(self) {
        let error = self
            .stop_control()
            .stop_error()
            .unwrap_or(InferenceError::Cancelled);
        self.respond(Err(error.to_string()));
    }
}

impl RequestQueue {
    /// Enqueue a typed request under the shared pending bound.
    pub async fn enqueue_typed_controlled(
        &self,
        model_id: String,
        input: InferenceInput,
        capability: InferenceCapability,
        params: InferenceParams,
        cancelled: Arc<AtomicBool>,
    ) -> Result<(u64, TypedResponseRx), QueueError> {
        let queue = self.queue.lock().await;
        let streaming = self.streaming.lock().await;
        let mut typed = self.typed.lock().await;
        if queue.len() + streaming.len() + typed.len() >= self.config.max_pending {
            return Err(QueueError::QueueFull);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let deadline = params
            .timeout_ms
            .map(|ms| Instant::now() + Duration::from_millis(ms));
        let (tx, rx) = tokio::sync::oneshot::channel();
        typed.push_back(TypedQueuedRequest {
            id,
            model_id,
            input,
            capability,
            params,
            deadline,
            cancelled,
            response_tx: Some(tx),
        });
        drop((queue, streaming, typed));
        self.notify.notify_one();
        Ok((id, rx))
    }

    /// Dequeue typed work, including cancelled or expired requests (the
    /// worker answers those explicitly instead of dropping them silently).
    pub async fn dequeue_typed(&self) -> Option<TypedQueuedRequest> {
        self.typed.lock().await.pop_front()
    }

    /// Current number of queued typed requests.
    pub async fn typed_len(&self) -> usize {
        self.typed.lock().await.len()
    }
}
