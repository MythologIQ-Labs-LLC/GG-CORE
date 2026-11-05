//! Per-request stop control: caller cancellation plus a wall-clock deadline.
//!
//! One value carries both signals from the caller (IPC handler, queue, or the
//! embedded façade) down to the backend's cooperative checkpoints: between
//! prompt-prefill decode chunks and before every generated token. A request
//! that stops for either reason must end in an explicit error, never in a
//! success-shaped partial result.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::inference_types::InferenceError;

/// Cancellation flag and deadline for one inference request.
#[derive(Debug, Clone, Default)]
pub struct StopControl {
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    timeout_ms: u64,
}

impl StopControl {
    /// Control with an optional shared cancel flag and an absolute deadline.
    /// `timeout_ms` is only used to describe the deadline in the error.
    pub fn new(
        cancelled: Option<Arc<AtomicBool>>,
        deadline: Option<Instant>,
        timeout_ms: u64,
    ) -> Self {
        Self {
            cancelled,
            deadline,
            timeout_ms,
        }
    }

    /// Control whose deadline starts now from a request's `timeout_ms`.
    pub fn from_timeout(cancelled: Option<Arc<AtomicBool>>, timeout_ms: Option<u64>) -> Self {
        let deadline =
            timeout_ms.and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
        Self::new(cancelled, deadline, timeout_ms.unwrap_or(0))
    }

    /// True once the caller cancelled the request.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    /// True once the request's deadline has passed.
    pub fn is_expired(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    /// Checkpoint test used by the backend.
    pub fn should_stop(&self) -> bool {
        self.is_cancelled() || self.is_expired()
    }

    /// The explicit failure for a stopped request; cancellation wins when both
    /// apply because it is the caller's own decision.
    pub fn stop_error(&self) -> Option<InferenceError> {
        if self.is_cancelled() {
            Some(InferenceError::Cancelled)
        } else if self.is_expired() {
            Some(InferenceError::DeadlineExceeded {
                timeout_ms: self.timeout_ms,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_control_never_stops() {
        let control = StopControl::default();
        assert!(!control.should_stop());
        assert!(control.stop_error().is_none());
    }

    #[test]
    fn cancel_flag_stops_with_cancelled_error() {
        let flag = Arc::new(AtomicBool::new(false));
        let control = StopControl::from_timeout(Some(Arc::clone(&flag)), None);
        assert!(!control.should_stop());
        flag.store(true, Ordering::Release);
        assert!(control.should_stop());
        assert!(matches!(
            control.stop_error(),
            Some(InferenceError::Cancelled)
        ));
    }

    #[test]
    fn elapsed_deadline_stops_with_deadline_error() {
        let past = Instant::now() - Duration::from_millis(5);
        let control = StopControl::new(None, Some(past), 350);
        assert!(control.should_stop());
        match control.stop_error() {
            Some(InferenceError::DeadlineExceeded { timeout_ms }) => assert_eq!(timeout_ms, 350),
            other => panic!("expected deadline error, got {other:?}"),
        }
    }

    #[test]
    fn cancellation_takes_precedence_over_deadline() {
        let flag = Arc::new(AtomicBool::new(true));
        let past = Instant::now() - Duration::from_millis(5);
        let control = StopControl::new(Some(flag), Some(past), 10);
        assert!(matches!(
            control.stop_error(),
            Some(InferenceError::Cancelled)
        ));
    }

    #[test]
    fn future_deadline_does_not_stop() {
        let control = StopControl::from_timeout(None, Some(60_000));
        assert!(!control.should_stop());
    }
}
