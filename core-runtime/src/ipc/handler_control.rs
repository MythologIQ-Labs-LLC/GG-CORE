//! Request-control helpers shared by every queued IPC request kind
//! (text generation and typed embedding/classification), so cancellation,
//! deadline and admission semantics cannot drift between them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::engine::inference::InferenceError;
use crate::ipc::protocol::{InferenceErrorCode, RequestId};
use crate::scheduler::QueueError;

/// Stops the queued request if the awaiting future is dropped (for example
/// when its connection task ends), so no work runs for a vanished caller.
/// Setting it after completion is harmless: the flag belongs to one request.
pub(super) struct CancelOnDrop(pub(super) Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Map an enqueue failure to its wire code. An over-long prompt is a
/// permanent input error, not a retriable admission rejection like a full
/// queue.
pub(super) fn enqueue_failure_code(e: &QueueError) -> InferenceErrorCode {
    match e {
        QueueError::ContextTooLarge { .. } => InferenceErrorCode::InputInvalid,
        _ => InferenceErrorCode::AdmissionRejected,
    }
}

/// Map a caller stop (cancel or deadline) to its wire code.
pub(super) fn stop_code(error: &InferenceError) -> InferenceErrorCode {
    match error {
        InferenceError::DeadlineExceeded { .. } => InferenceErrorCode::DeadlineExceeded,
        _ => InferenceErrorCode::Cancelled,
    }
}

/// Generation-path response for an enqueue failure.
pub(super) fn enqueue_failure(
    id: RequestId,
    e: QueueError,
) -> crate::ipc::protocol::InferenceResponse {
    let code = enqueue_failure_code(&e);
    crate::ipc::protocol::InferenceResponse::error_coded(id, e.to_string(), code)
}

/// Generation-path response for a caller stop.
pub(super) fn stopped(
    id: RequestId,
    error: InferenceError,
) -> crate::ipc::protocol::InferenceResponse {
    let code = stop_code(&error);
    crate::ipc::protocol::InferenceResponse::error_coded(id, error.to_string(), code)
}
