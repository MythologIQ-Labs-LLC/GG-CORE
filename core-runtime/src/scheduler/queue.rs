//! Request queue management.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Notify};

use super::priority::{Priority, PriorityQueue};
use super::streaming_queue::StreamingQueuedRequest;
use crate::engine::inference::InferenceResult;
use crate::engine::InferenceParams;

pub use super::queued_request::{QueuedRequest, ResponseTx};

/// Receiver half for awaiting inference results.
pub type ResponseRx = tokio::sync::oneshot::Receiver<Result<InferenceResult, String>>;

/// Conservative bytes-per-token estimate for tier-1 context check.
const BYTES_PER_TOKEN_ESTIMATE: usize = 4;

/// Configuration for request queue.
#[derive(Debug, Clone)]
pub struct RequestQueueConfig {
    pub max_pending: usize,
    /// Maximum context length in tokens. Used for tier-1 heuristic
    /// rejection at enqueue time (prompt_bytes / 4 > max_context).
    pub max_context_tokens: usize,
}

impl Default for RequestQueueConfig {
    fn default() -> Self {
        Self { max_pending: 256, max_context_tokens: 4096 }
    }
}

/// Thread-safe request queue with priority support.
pub struct RequestQueue {
    queue: Arc<Mutex<PriorityQueue<QueuedRequest>>>,
    streaming: Arc<Mutex<VecDeque<StreamingQueuedRequest>>>,
    next_id: AtomicU64,
    config: RequestQueueConfig,
    notify: Arc<Notify>,
}

impl RequestQueue {
    pub fn new(config: RequestQueueConfig) -> Self {
        Self {
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            streaming: Arc::new(Mutex::new(VecDeque::new())),
            next_id: AtomicU64::new(1),
            config,
            notify: Arc::new(Notify::new()),
        }
    }

    /// Enqueue a new request. Returns request ID and queue position.
    pub async fn enqueue(
        &self,
        model_id: String,
        prompt: String,
        params: InferenceParams,
        priority: Priority,
    ) -> Result<(u64, usize), QueueError> {
        self.enqueue_inner(model_id, prompt, params, priority, None).await
    }

    /// Enqueue with a response channel. Returns (id, receiver).
    pub async fn enqueue_with_response(
        &self,
        model_id: String,
        prompt: String,
        params: InferenceParams,
        priority: Priority,
    ) -> Result<(u64, ResponseRx), QueueError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (id, _pos) = self
            .enqueue_inner(model_id, prompt, params, priority, Some(tx))
            .await?;
        Ok((id, rx))
    }

    async fn enqueue_inner(
        &self,
        model_id: String,
        prompt: String,
        params: InferenceParams,
        priority: Priority,
        response_tx: Option<ResponseTx>,
    ) -> Result<(u64, usize), QueueError> {
        check_context(&prompt, self.config.max_context_tokens)?;

        let mut queue = self.queue.lock().await;
        let streaming = self.streaming.lock().await;
        let total = queue.len() + streaming.len();
        drop(streaming);
        if total >= self.config.max_pending {
            return Err(QueueError::QueueFull);
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = QueuedRequest::with_tx(
            id, model_id, prompt, params, response_tx,
        );
        let position = queue.len();
        queue.push(request, priority);
        drop(queue);

        self.notify.notify_one();
        Ok((id, position))
    }

    /// Cancel a pending request by ID. Returns true if found.
    pub async fn cancel(&self, request_id: u64) -> bool {
        let queue = self.queue.lock().await;
        for request in queue.iter() {
            if request.id == request_id {
                request.cancel();
                return true;
            }
        }
        false
    }

    /// Dequeue the highest priority request, skipping cancelled/expired.
    pub async fn dequeue(&self) -> Option<QueuedRequest> {
        let mut queue = self.queue.lock().await;
        loop {
            let request = queue.pop()?;
            if request.is_cancelled() || request.is_expired() {
                continue;
            }
            return Some(request);
        }
    }

    /// Wait for a notification then dequeue.
    pub async fn wait_and_dequeue(&self) -> Option<QueuedRequest> {
        loop {
            if let Some(req) = self.dequeue().await {
                return Some(req);
            }
            self.notify.notified().await;
        }
    }

    /// Wake the worker (used during shutdown).
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// Current queue length.
    pub async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }

    /// Check if queue is empty.
    pub async fn is_empty(&self) -> bool {
        self.queue.lock().await.is_empty()
    }

    /// Enqueue a streaming request. Returns the request ID.
    pub async fn enqueue_streaming(
        &self,
        model_id: String,
        prompt: String,
        config: crate::engine::InferenceConfig,
        token_sender: super::streaming_queue::TokenStreamTx,
    ) -> Result<u64, QueueError> {
        check_context(&prompt, self.config.max_context_tokens)?;

        let queue = self.queue.lock().await;
        let mut streaming = self.streaming.lock().await;
        if queue.len() + streaming.len() >= self.config.max_pending {
            return Err(QueueError::QueueFull);
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        streaming.push_back(StreamingQueuedRequest {
            id,
            model_id,
            prompt,
            config,
            enqueued_at: Instant::now(),
            deadline: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            token_sender,
        });
        drop(streaming);
        drop(queue);

        self.notify.notify_one();
        Ok(id)
    }

    /// Dequeue a streaming request, skipping cancelled/expired.
    pub async fn dequeue_streaming(
        &self,
    ) -> Option<StreamingQueuedRequest> {
        let mut streaming = self.streaming.lock().await;
        loop {
            let request = streaming.pop_front()?;
            if request.is_cancelled() || request.is_expired() {
                continue;
            }
            return Some(request);
        }
    }

    /// Current number of queued streaming requests.
    pub async fn streaming_len(&self) -> usize {
        self.streaming.lock().await.len()
    }
}

fn check_context(prompt: &str, max: usize) -> Result<(), QueueError> {
    let estimated = prompt.len() / BYTES_PER_TOKEN_ESTIMATE;
    if estimated > max {
        return Err(QueueError::ContextTooLarge {
            estimated_tokens: estimated,
            max,
        });
    }
    Ok(())
}

#[derive(Debug)]
pub enum QueueError {
    QueueFull,
    ContextTooLarge { estimated_tokens: usize, max: usize },
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull => write!(f, "request queue is full"),
            Self::ContextTooLarge { estimated_tokens, max } => {
                write!(f, "prompt too large: ~{estimated_tokens} tokens (max {max})")
            }
        }
    }
}

impl std::error::Error for QueueError {}
