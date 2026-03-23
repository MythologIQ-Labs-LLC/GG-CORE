//! Streaming request queue types.
//!
//! Streaming requests go through the worker loop just like regular
//! requests, but deliver tokens via an mpsc channel instead of a
//! oneshot response.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::engine::{InferenceConfig, TokenStreamSender};

/// Channel type for streaming token delivery.
pub type TokenStreamTx = TokenStreamSender;

/// A queued streaming inference request.
pub struct StreamingQueuedRequest {
    pub id: u64,
    pub model_id: String,
    pub prompt: String,
    pub config: InferenceConfig,
    pub enqueued_at: Instant,
    pub deadline: Option<Instant>,
    pub cancelled: Arc<AtomicBool>,
    pub token_sender: TokenStreamTx,
}

impl StreamingQueuedRequest {
    /// Check if request has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Check if request has exceeded its deadline.
    pub fn is_expired(&self) -> bool {
        self.deadline.map_or(false, |d| Instant::now() > d)
    }
}
