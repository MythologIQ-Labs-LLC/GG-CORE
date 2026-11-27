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

use super::handler_control::{enqueue_failure, stopped, CancelOnDrop};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::{classify_worker_error, IpcHandler};
use crate::engine::inference::{InferenceError, InferenceResult};
use crate::ipc::protocol::{InferenceErrorCode, InferenceRequest, InferenceResponse, RequestId};
use crate::scheduler::{Priority, ResponseRx};

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
        let deadline = match timeout_ms {
            Some(ms) => match tokio::time::Instant::now().checked_add(Duration::from_millis(ms)) {
                Some(deadline) => Some(deadline),
                None => {
                    return InferenceResponse::error_coded(
                        id,
                        "timeout_ms exceeds the supported clock range".into(),
                        InferenceErrorCode::InputInvalid,
                    );
                }
            },
            None => None,
        };
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
        let (queue_id, rx) = match enqueued {
            Ok((queue_id, rx)) => (queue_id, rx),
            Err(e) => return enqueue_failure(id, e),
        };
        let stop = |error: InferenceError| {
            flag.store(true, Ordering::Release);
            // #184 stop trace S2; the worker's S4 carries the same `queue_id`.
            // A deadline S2 records how late this answer runs after the
            // deadline instant: the backend observes the same instant on its
            // own clock, so S3 may precede S2 by exactly that lag (#184 R1).
            match (&error, deadline) {
                (InferenceError::Cancelled, _) => {
                    crate::telemetry::stop_trace::answered(id.0, Some(queue_id), true, false);
                }
                (_, Some(due)) => {
                    let lag = tokio::time::Instant::now().saturating_duration_since(due);
                    let lag_ms = lag.as_secs_f64() * 1000.0;
                    crate::telemetry::stop_trace::deadline_answered(
                        id.0,
                        Some(queue_id),
                        false,
                        lag_ms,
                    );
                }
                (_, None) => {
                    crate::telemetry::stop_trace::answered(id.0, Some(queue_id), false, false);
                }
            }
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
