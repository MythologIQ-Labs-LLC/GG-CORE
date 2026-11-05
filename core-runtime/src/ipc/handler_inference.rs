//! Non-streaming inference with caller cancellation and a wall-clock deadline.
//!
//! The request is enqueued with a cancel flag owned here. The flag is set when
//! the connection cancels the request (`cancel_request` or disconnect), when
//! `timeout_ms` elapses, or when this future is dropped; the worker observes
//! it before execution, between prompt-prefill chunks, and per generated
//! token, so the request stops and releases its context, queue slot, and
//! memory reservation. The caller always receives an explicit
//! `cancelled` / `deadline_exceeded` failure, never a partial success.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::{classify_worker_error, IpcHandler};
use crate::engine::inference::{InferenceError, InferenceResult};
use crate::ipc::protocol::{InferenceErrorCode, InferenceRequest, InferenceResponse, RequestId};
use crate::scheduler::{Priority, QueueError, ResponseRx};

/// Stops the queued request if the awaiting future is dropped (for example
/// when its connection task ends), so no work runs for a vanished caller.
/// Setting it after completion is harmless: the flag belongs to one request.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl IpcHandler {
    /// Process one non-streaming inference request under `cancel` and the
    /// request's own `timeout_ms`.
    pub(crate) async fn handle_inference_controlled(
        &self,
        request: InferenceRequest,
        cancel: CancellationToken,
    ) -> InferenceResponse {
        let id = request.request_id;
        let Some(_guard) = self.shutdown.track() else {
            let msg = "Server is shutting down".to_string();
            return InferenceResponse::error_coded(id, msg, InferenceErrorCode::ShuttingDown);
        };
        if let Err(e) = request.validate() {
            return InferenceResponse::error_coded(
                id,
                e.to_string(),
                InferenceErrorCode::InputInvalid,
            );
        }
        if let Err(e) = request.parameters.validate() {
            return InferenceResponse::error_coded(
                id,
                e.to_string(),
                InferenceErrorCode::InputInvalid,
            );
        }
        let timeout_ms = request.parameters.timeout_ms;
        let deadline = timeout_ms.map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
        let flag = Arc::new(AtomicBool::new(false));
        let _stop_on_drop = CancelOnDrop(Arc::clone(&flag));

        let enqueued = self
            .queue
            .enqueue_controlled(
                request.model_id,
                request.prompt,
                request.parameters,
                Priority::Normal,
                Arc::clone(&flag),
            )
            .await;
        let rx = match enqueued {
            Ok((_queue_id, rx)) => rx,
            Err(e) => return enqueue_failure(id, e),
        };
        let stop = |error: InferenceError| {
            flag.store(true, Ordering::Release);
            stopped(id, error)
        };
        let expiry = async {
            match deadline {
                Some(d) => tokio::time::sleep_until(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            // A result that is already available wins over a racing stop.
            biased;
            outcome = rx => worker_outcome(id, outcome),
            () = cancel.cancelled() => stop(InferenceError::Cancelled),
            () = expiry => stop(InferenceError::DeadlineExceeded {
                timeout_ms: timeout_ms.unwrap_or(0),
            }),
        }
    }
}

fn enqueue_failure(id: RequestId, e: QueueError) -> InferenceResponse {
    // An over-long prompt is a permanent input error, not a retriable
    // admission rejection like a full queue.
    let code = match &e {
        QueueError::ContextTooLarge { .. } => InferenceErrorCode::InputInvalid,
        _ => InferenceErrorCode::AdmissionRejected,
    };
    InferenceResponse::error_coded(id, e.to_string(), code)
}

fn stopped(id: RequestId, error: InferenceError) -> InferenceResponse {
    let code = match error {
        InferenceError::DeadlineExceeded { .. } => InferenceErrorCode::DeadlineExceeded,
        _ => InferenceErrorCode::Cancelled,
    };
    InferenceResponse::error_coded(id, error.to_string(), code)
}

type WorkerOutcome = <ResponseRx as std::future::Future>::Output;

fn worker_outcome(id: RequestId, outcome: WorkerOutcome) -> InferenceResponse {
    match outcome {
        Ok(Ok(InferenceResult {
            output,
            tokens_generated,
            finished,
        })) => InferenceResponse::success(id, output, tokens_generated, finished),
        Ok(Err(e)) => {
            let code = classify_worker_error(&e);
            InferenceResponse::error_coded(id, e, code)
        }
        Err(_) => InferenceResponse::error_coded(
            id,
            "worker dropped response channel".into(),
            InferenceErrorCode::ExecutionFailed,
        ),
    }
}
