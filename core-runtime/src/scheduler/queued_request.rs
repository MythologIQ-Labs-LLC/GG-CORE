//! Queued request type for the inference queue.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engine::inference::InferenceResult;
use crate::engine::InferenceParams;

/// Response channel type (re-exported for queue).
pub type ResponseTx = tokio::sync::oneshot::Sender<Result<InferenceResult, String>>;

/// A queued inference request with timeout and cancellation support.
pub struct QueuedRequest {
    pub id: u64,
    pub model_id: String,
    /// Text prompt for inference.
    pub prompt: String,
    pub params: InferenceParams,
    pub enqueued_at: Instant,
    pub deadline: Option<Instant>,
    pub cancelled: Arc<AtomicBool>,
    /// Channel for sending the result back to the caller.
    pub response_tx: Option<ResponseTx>,
}

impl std::fmt::Debug for QueuedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueuedRequest")
            .field("id", &self.id)
            .field("model_id", &self.model_id)
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl QueuedRequest {
    /// Create a new queued request. Used for testing and batch processing.
    pub fn new(
        id: u64,
        model_id: String,
        prompt: String,
        params: InferenceParams,
    ) -> Self {
        Self::with_tx(id, model_id, prompt, params, None)
    }

    /// Create a queued request with an optional response channel.
    pub fn with_tx(
        id: u64,
        model_id: String,
        prompt: String,
        params: InferenceParams,
        response_tx: Option<ResponseTx>,
    ) -> Self {
        let enqueued_at = Instant::now();
        let deadline = params.timeout_ms.map(|ms| enqueued_at + Duration::from_millis(ms));
        Self {
            id,
            model_id,
            prompt,
            params,
            enqueued_at,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
            response_tx,
        }
    }

    /// Check if request has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Check if request has exceeded its deadline.
    pub fn is_expired(&self) -> bool {
        self.deadline.map_or(false, |d| Instant::now() > d)
    }

    /// Mark the request as cancelled.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Get a closure that checks cancellation (for passing to backends).
    pub fn cancel_check(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }
}
