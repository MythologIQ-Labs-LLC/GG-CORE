//! Request queue management.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::priority::{Priority, PriorityQueue};
use crate::engine::InferenceParams;

/// Configuration for request queue.
#[derive(Debug, Clone)]
pub struct RequestQueueConfig {
    pub max_pending: usize,
}

impl Default for RequestQueueConfig {
    fn default() -> Self {
        Self { max_pending: 256 }
    }
}

/// A queued inference request with timeout and cancellation support.
#[derive(Debug)]
pub struct QueuedRequest {
    pub id: u64,
    pub model_id: String,
    /// Text prompt for inference.
    pub prompt: String,
    pub params: InferenceParams,
    pub enqueued_at: Instant,
    pub deadline: Option<Instant>,
    cancelled: Arc<AtomicBool>,
}

impl Clone for QueuedRequest {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            model_id: self.model_id.clone(),
            prompt: self.prompt.clone(),
            params: self.params.clone(),
            enqueued_at: self.enqueued_at,
            deadline: self.deadline,
            cancelled: Arc::clone(&self.cancelled),
        }
    }
}

impl QueuedRequest {
    /// Create a new queued request. Used for testing and batch processing.
    pub fn new(id: u64, model_id: String, prompt: String, params: InferenceParams) -> Self {
        let enqueued_at = Instant::now();
        let deadline = params
            .timeout_ms
            .map(|ms| enqueued_at + Duration::from_millis(ms));
        Self {
            id,
            model_id,
            prompt,
            params,
            enqueued_at,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if request has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Check if request has exceeded its deadline.
    pub fn is_expired(&self) -> bool {
        self.deadline.map_or(false, |d| Instant::now() > d)
    }

    /// Mark the request as cancelled.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Thread-safe request queue with priority support.
pub struct RequestQueue {
    queue: Arc<Mutex<PriorityQueue<QueuedRequest>>>,
    next_id: AtomicU64,
    config: RequestQueueConfig,
}

impl RequestQueue {
    pub fn new(config: RequestQueueConfig) -> Self {
        Self {
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            next_id: AtomicU64::new(1),
            config,
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
        let mut queue = self.queue.lock().await;

        if queue.len() >= self.config.max_pending {
            return Err(QueueError::QueueFull);
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let enqueued_at = Instant::now();
        let deadline = params
            .timeout_ms
            .map(|ms| enqueued_at + Duration::from_millis(ms));
        let request = QueuedRequest {
            id,
            model_id,
            prompt,
            params,
            enqueued_at,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let position = queue.len();
        queue.push(request, priority);

        Ok((id, position))
    }

    /// Cancel a pending request by ID. Returns true if found and cancelled.
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
                continue; // Skip cancelled/expired requests
            }
            return Some(request);
        }
    }

    /// Current queue length.
    pub async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }

    /// Check if queue is empty.
    pub async fn is_empty(&self) -> bool {
        self.queue.lock().await.is_empty()
    }
}

#[derive(Debug)]
pub enum QueueError {
    QueueFull,
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull => write!(f, "request queue is full"),
        }
    }
}

impl std::error::Error for QueueError {}
