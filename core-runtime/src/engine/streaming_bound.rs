//! Time bound on every token-stream send
//! (`docs/integration/V1_SCHEDULER_HOL_BLOCKING_PLAN.md`, Phase 1).
//!
//! A consumer that stops reading must never park the producer (and with it
//! the scheduler worker) indefinitely. Each send waits at most `stall`, and
//! never past the request deadline.

use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, error::TrySendError};

use super::streaming::{StreamItem, StreamSendError};

/// Default `GG_CORE_STREAM_STALL_MS`.
pub const DEFAULT_STREAM_STALL: Duration = Duration::from_millis(5_000);
/// Lower bound for the stall setting.
pub const MIN_STREAM_STALL: Duration = Duration::from_millis(100);

/// How long a stream send may wait for its consumer.
#[derive(Debug, Clone, Copy)]
pub struct StreamBound {
    /// Longest single wait for channel capacity.
    pub stall: Duration,
    /// Request deadline; no send waits past it.
    pub deadline: Option<Instant>,
}

impl Default for StreamBound {
    fn default() -> Self {
        Self {
            stall: DEFAULT_STREAM_STALL,
            deadline: None,
        }
    }
}

impl StreamBound {
    /// Bound for one request: `stall` clamped to `[MIN_STREAM_STALL,
    /// timeout]`, deadline `now + timeout`.
    pub fn for_request(stall: Duration, timeout: Duration) -> Self {
        let ceiling = timeout.max(MIN_STREAM_STALL);
        Self {
            stall: stall.clamp(MIN_STREAM_STALL, ceiling),
            deadline: Instant::now().checked_add(timeout),
        }
    }

    /// When the next send must have completed, and whether that limit is the
    /// stall (rather than the deadline).
    fn send_by(&self) -> (Instant, bool) {
        let stall_at = Instant::now() + self.stall;
        match self.deadline {
            Some(d) if d < stall_at => (d, false),
            _ => (stall_at, true),
        }
    }
}

/// Send one frame within the bound. A deadline already passed still delivers
/// the frame if capacity is free (so a final terminal is not lost).
pub(super) async fn bounded_send(
    tx: &mpsc::Sender<StreamItem>,
    bound: &StreamBound,
    item: StreamItem,
) -> Result<(), StreamSendError> {
    let (by, stall_limited) = bound.send_by();
    let expired = if stall_limited {
        StreamSendError::Stalled
    } else {
        StreamSendError::Deadline
    };
    if by <= Instant::now() {
        return tx.try_send(item).map_err(|e| match e {
            TrySendError::Full(_) => expired,
            TrySendError::Closed(_) => StreamSendError::Closed,
        });
    }
    match tokio::time::timeout_at(by.into(), tx.send(item)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(StreamSendError::Closed),
        Err(_) => Err(expired),
    }
}

#[cfg(test)]
#[path = "streaming_bound_tests.rs"]
mod tests;
