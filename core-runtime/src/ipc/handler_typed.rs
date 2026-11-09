//! Typed (embedding/classification) request handling with caller
//! cancellation and a wall-clock deadline, through the shared queue.
//!
//! Same contract as non-streaming generation: validated before enqueue,
//! stopped by `cancel_request`, disconnect, `timeout_ms` or drop, and always
//! answered with an explicit result or coded error, never a partial success.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::handler_control::{enqueue_failure_code, stop_code, CancelOnDrop};
use super::{classify_worker_error, IpcHandler};
use crate::engine::inference::InferenceError;
use crate::engine::{InferenceOutput, InferenceParams};
use crate::ipc::protocol::{InferenceErrorCode, RequestId};
use crate::ipc::protocol_typed::{TypedInferenceRequest, TypedInferenceResponse, TypedResult};

impl IpcHandler {
    /// Process one typed request under `cancel` and its own `timeout_ms`.
    pub(crate) async fn handle_typed_controlled(
        &self,
        request: TypedInferenceRequest,
        cancel: CancellationToken,
    ) -> TypedInferenceResponse {
        let id = request.request_id;
        let fail = |message: String, code| TypedInferenceResponse::error(id, message, code);
        let Some(_guard) = self.shutdown.track() else {
            return fail(
                "Server is shutting down".into(),
                InferenceErrorCode::ShuttingDown,
            );
        };
        let params = InferenceParams {
            timeout_ms: request.timeout_ms,
            ..InferenceParams::default()
        };
        if let Err(e) = params.validate() {
            return fail(e.to_string(), InferenceErrorCode::InputInvalid);
        }
        let input = request.input.into_engine();
        if let Err(e) = input.validate() {
            return fail(e.to_string(), InferenceErrorCode::InputInvalid);
        }
        let deadline = params
            .timeout_ms
            .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
        let flag = Arc::new(AtomicBool::new(false));
        let _stop_on_drop = CancelOnDrop(Arc::clone(&flag));
        let enqueued = self
            .queue
            .enqueue_typed_controlled(
                request.model_id,
                input,
                request.task.capability(),
                params.clone(),
                Arc::clone(&flag),
            )
            .await;
        let rx = match enqueued {
            Ok((_queue_id, rx)) => rx,
            Err(e) => return fail(e.to_string(), enqueue_failure_code(&e)),
        };
        let stop = |error: InferenceError| {
            flag.store(true, Ordering::Release);
            fail(error.to_string(), stop_code(&error))
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
            outcome = rx => typed_outcome(id, outcome),
            () = cancel.cancelled() => stop(InferenceError::Cancelled),
            () = expiry => stop(InferenceError::DeadlineExceeded {
                timeout_ms: params.timeout_ms.unwrap_or(0),
            }),
        }
    }
}

type Outcome = Result<Result<InferenceOutput, String>, tokio::sync::oneshot::error::RecvError>;

fn typed_outcome(id: RequestId, outcome: Outcome) -> TypedInferenceResponse {
    match outcome {
        Ok(Ok(output)) => match TypedResult::from_engine(output) {
            Ok(result) => TypedInferenceResponse::success(id, result),
            Err(e) => TypedInferenceResponse::error(id, e, InferenceErrorCode::ExecutionFailed),
        },
        Ok(Err(e)) => {
            let code = typed_error_code(&e);
            TypedInferenceResponse::error(id, e, code)
        }
        Err(_) => TypedInferenceResponse::error(
            id,
            "worker dropped response channel".into(),
            InferenceErrorCode::ExecutionFailed,
        ),
    }
}

/// Capability mismatches are explicit; everything else uses the shared
/// worker-error classification.
fn typed_error_code(message: &str) -> InferenceErrorCode {
    if message.contains("not supported by selected model") {
        InferenceErrorCode::UnsupportedTask
    } else {
        classify_worker_error(message)
    }
}
