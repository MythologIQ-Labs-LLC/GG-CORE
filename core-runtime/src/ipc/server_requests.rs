//! Admission and spawning of queued, cancellable requests on a connection.
//! Shared by text generation and typed inference so both enforce the same
//! fail-closed auth (401, then close), duplicate-ID (409) and cancel rules.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::{encode_message, register_request, ActiveRequests};
use crate::ipc::auth::SessionToken;
use crate::ipc::conn_writer::ConnWriter;
use crate::ipc::handler::IpcHandler;
use crate::ipc::protocol::IpcMessage;
use crate::ipc::protocol_typed::TypedInferenceRequest;

/// What the connection loop does after admitting a request.
pub(super) enum Admission {
    Run(CancellationToken),
    Continue,
    Close,
}

pub(super) async fn admit<W: AsyncWriteExt + Unpin>(
    handler: &IpcHandler,
    session: Option<&SessionToken>,
    writer: &Arc<ConnWriter<W>>,
    active: &ActiveRequests,
    request_id: u64,
) -> Admission {
    if handler.require_auth(session).await.is_err() {
        let err = r#"{"type":"error","code":401,"message":"Not authenticated"}"#;
        let _ = writer.write_frame(err.as_bytes()).await;
        return Admission::Close;
    }
    match register_request(active, request_id).await {
        Some(cancel) => Admission::Run(cancel),
        None => {
            let err = r#"{"type":"error","code":409,"message":"Duplicate active request ID"}"#;
            let _ = writer.write_frame(err.as_bytes()).await;
            Admission::Continue
        }
    }
}

/// Run one typed request beside the reader and write its response frame.
/// The ID is released before the write, as for generation.
pub(super) fn spawn_typed<W: AsyncWriteExt + Unpin + Send + 'static>(
    handler: Arc<IpcHandler>,
    writer: Arc<ConnWriter<W>>,
    active: ActiveRequests,
    request: TypedInferenceRequest,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        let id = request.request_id;
        let response = handler.handle_typed_controlled(request, cancel).await;
        active.lock().await.remove(&id.0);
        let message = IpcMessage::TypedInferenceResponse(response);
        let bytes = match encode_message(&message) {
            Ok(bytes) => bytes,
            Err(_) => {
                let fallback = crate::ipc::protocol_typed::TypedInferenceResponse::error(
                    id,
                    "typed response could not be encoded".into(),
                    crate::ipc::protocol::InferenceErrorCode::ExecutionFailed,
                );
                match encode_message(&IpcMessage::TypedInferenceResponse(fallback)) {
                    Ok(bytes) => bytes,
                    Err(_) => return,
                }
            }
        };
        let _ = writer.write_frame(&bytes).await;
    });
}
