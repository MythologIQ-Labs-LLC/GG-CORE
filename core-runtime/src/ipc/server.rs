//! IPC server loop for accepting and processing connections.
//!
//! Uses platform-specific transports:
//! - Unix: `tokio::net::UnixListener` (file-based sockets)
//! - Windows: `tokio::net::windows::named_pipe` (named pipes)
//!
//! All connections use length-prefixed framing (4-byte LE + payload)
//! matching the CLI client protocol in `cli::ipc_client`. Every write goes
//! through the connection's single [`ConnWriter`].

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::conn_writer::ConnWriter;
use super::connections::{ConnectionPool, OwnedConnectionGuard};
use super::handler::{HandlerError, IpcHandler, StreamSender};
use super::protocol::{decode_message, encode_message, IpcMessage, StreamChunk};
use super::stream_bridge::IpcStreamBridge;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Frame too large: {size} bytes (max {max})")]
    FrameTooLarge { size: usize, max: usize },
}

/// Read a length-prefixed frame from an async reader.
async fn read_frame<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    max_frame_size: usize,
) -> Result<Vec<u8>, ServerError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;

    let frame_len = u32::from_le_bytes(len_buf) as usize;
    if frame_len > max_frame_size {
        return Err(ServerError::FrameTooLarge {
            size: frame_len,
            max: max_frame_size,
        });
    }

    let mut buf = vec![0u8; frame_len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Handle one IPC connection: read requests, dispatch, write responses.
/// Supports both synchronous request/response and streaming inference.
async fn handle_connection<S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static>(
    stream: S,
    handler: Arc<IpcHandler>,
    _guard: OwnedConnectionGuard,
    max_frame_size: usize,
) {
    let (mut read_half, write_half) = tokio::io::split(stream);
    let write_half = Arc::new(ConnWriter::new(write_half, handler.stream_stall()));
    let mut session: Option<super::auth::SessionToken> = None;
    let active_requests: ActiveRequests = Arc::new(Mutex::new(HashMap::new()));
    let release_waits = release::ReleaseWaits::default();

    loop {
        // A poisoned writer (stalled or failed frame write) ends the
        // connection: nothing more can be framed safely on it.
        let read = tokio::select! {
            biased;
            () = write_half.closed().cancelled() => break,
            read = read_frame(&mut read_half, max_frame_size) => read,
        };
        let request_bytes = match read {
            Ok(bytes) => bytes,
            Err(ServerError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => {
                eprintln!("Connection read error: {}", e);
                break;
            }
        };

        // Parse message to detect streaming vs non-streaming
        let message = match decode_message(&request_bytes) {
            Ok(m) => m,
            Err(e) => {
                let err = format!(r#"{{"type":"error","code":400,"message":"{}"}}"#, e);
                let _ = write_half.write_frame(err.as_bytes()).await;
                continue;
            }
        };

        match message {
            // Streaming inference request
            IpcMessage::InferenceRequest(ref req) if req.parameters.stream => {
                if let Some(ref sess) = session {
                    // process_streaming validates this connection's session
                    // before enqueueing work; don't double-count the request.
                    let id = req.request_id.0;
                    let cancel = CancellationToken::new();
                    {
                        let mut streams = active_requests.lock().await;
                        if streams.contains_key(&id) {
                            drop(streams);
                            let err = r#"{"type":"error","code":409,"message":"Duplicate active request ID"}"#;
                            let _ = write_half.write_frame(err.as_bytes()).await;
                            continue;
                        }
                        streams.insert(id, cancel.clone());
                    }
                    // Stream relay runs independently of the connection reader,
                    // allowing cancellation and other control frames to arrive.
                    let streams = Arc::clone(&active_requests);
                    let writer = Arc::clone(&write_half);
                    let stream_handler = Arc::clone(&handler);
                    let stream_session = (*sess).clone();
                    let stream_request = req.clone();
                    tokio::spawn(async move {
                        let bridge =
                            IpcStreamBridge::new(writer, stream_request.request_id, cancel.clone());
                        if let Err(e) = stream_handler
                            .process_streaming(
                                stream_request.clone(),
                                &stream_session,
                                &bridge,
                                cancel,
                            )
                            .await
                        {
                            let terminal =
                                StreamChunk::error(stream_request.request_id, e.to_string());
                            let _ = bridge.send(IpcMessage::StreamChunk(terminal)).await;
                        }
                        streams.lock().await.remove(&id);
                    });
                } else {
                    let err = r#"{"type":"error","code":401,"message":"Not authenticated"}"#;
                    let _ = write_half.write_frame(err.as_bytes()).await;
                }
            }

            // Non-streaming inference runs beside the reader, like a stream,
            // so cancel_request and disconnect reach it while it executes.
            IpcMessage::InferenceRequest(req) => {
                let id = req.request_id.0;
                match requests::admit(
                    &handler,
                    session.as_ref(),
                    &write_half,
                    &active_requests,
                    id,
                )
                .await
                {
                    requests::Admission::Run(cancel) => spawn_inference(
                        Arc::clone(&handler),
                        Arc::clone(&write_half),
                        Arc::clone(&active_requests),
                        req,
                        cancel,
                    ),
                    requests::Admission::Continue => continue,
                    requests::Admission::Close => break,
                }
            }

            // Typed embedding/classification (#171): same admission rules.
            IpcMessage::TypedInferenceRequest(req) => {
                let id = req.request_id.0;
                match requests::admit(
                    &handler,
                    session.as_ref(),
                    &write_half,
                    &active_requests,
                    id,
                )
                .await
                {
                    requests::Admission::Run(cancel) => requests::spawn_typed(
                        Arc::clone(&handler),
                        Arc::clone(&write_half),
                        Arc::clone(&active_requests),
                        req,
                        cancel,
                    ),
                    requests::Admission::Continue => continue,
                    requests::Admission::Close => break,
                }
            }

            // Cancellation is an authenticated operation on the originating
            // connection, including while its request is still running.
            IpcMessage::CancelRequest { request_id } => {
                if let Some(ref sess) = session {
                    if handler.auth.validate(sess).await.is_err() {
                        let err = r#"{"type":"error","code":401,"message":"Not authenticated"}"#;
                        let _ = write_half.write_frame(err.as_bytes()).await;
                        continue;
                    }
                    let streams = active_requests.lock().await;
                    let cancelled = if let Some(cancel) = streams.get(&request_id.0) {
                        cancel.cancel();
                        true
                    } else {
                        false
                    };
                    drop(streams);
                    let response = IpcMessage::CancelResponse {
                        request_id,
                        cancelled,
                    };
                    if let Ok(bytes) = encode_message(&response) {
                        let _ = write_half.write_frame(&bytes).await;
                    }
                } else {
                    let err = r#"{"type":"error","code":401,"message":"Not authenticated"}"#;
                    let _ = write_half.write_frame(err.as_bytes()).await;
                }
            }

            IpcMessage::ModelReleaseWaitRequest(req) => {
                let sess = session.as_ref();
                release::start(&handler, sess, &write_half, &release_waits, req).await;
            }

            // Non-streaming: use standard request/response processing
            _ => match handler.process(&request_bytes, session.as_ref()).await {
                Ok((response_bytes, new_session)) => {
                    if new_session.is_some() {
                        session = new_session;
                    }
                    if let Err(e) = write_half.write_frame(&response_bytes).await {
                        eprintln!("Connection write error: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    let code =
                        if matches!(&e, HandlerError::Auth(_) | HandlerError::NotAuthenticated) {
                            401
                        } else {
                            500
                        };
                    let err = format!(r#"{{"type":"error","code":{},"message":"{}"}}"#, code, e);
                    let _ = write_half.write_frame(err.as_bytes()).await;
                    break;
                }
            },
        }
    }
    // The last client reference disappeared. Stop inference rather than
    // leaving the worker generating for an abandoned stream or response.
    for cancel in active_requests.lock().await.values() {
        cancel.cancel();
    }
    release_waits.close();
    if write_half.is_poisoned() {
        write_half.shutdown().await;
    }
}

type ActiveRequests = Arc<Mutex<HashMap<u64, CancellationToken>>>;

/// Register an in-flight request ID on this connection; `None` if the ID is
/// already active.
async fn register_request(active: &ActiveRequests, id: u64) -> Option<CancellationToken> {
    let mut requests = active.lock().await;
    if requests.contains_key(&id) {
        return None;
    }
    let cancel = CancellationToken::new();
    requests.insert(id, cancel.clone());
    Some(cancel)
}

/// Run one non-streaming request and write its single response frame. The
/// ID is released before the write: a cancel arriving after completion
/// correctly reports `cancelled: false`.
fn spawn_inference<W: AsyncWriteExt + Unpin + Send + 'static>(
    handler: Arc<IpcHandler>,
    writer: Arc<ConnWriter<W>>,
    active: ActiveRequests,
    request: super::protocol::InferenceRequest,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        let id = request.request_id.0;
        let response = handler.handle_inference_controlled(request, cancel).await;
        active.lock().await.remove(&id);
        if let Ok(bytes) = encode_message(&IpcMessage::InferenceResponse(response)) {
            let _ = writer.write_frame(&bytes).await;
        }
    });
}

/// Accept one connection, acquire a guard, and spawn a handler task.
pub(super) fn spawn_connection<S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static>(
    stream: S,
    handler: &Arc<IpcHandler>,
    connections: &Arc<ConnectionPool>,
    max_frame_size: usize,
) {
    let guard = match connections.try_acquire_owned() {
        Some(g) => g,
        None => {
            eprintln!("Connection limit reached, rejecting client");
            super::admission::spawn_rejection(stream);
            return;
        }
    };
    let handler = Arc::clone(handler);
    tokio::spawn(async move {
        handle_connection(stream, handler, guard, max_frame_size).await;
    });
}

// Platform listeners (Unix socket / Windows named pipe) live in `listener.rs`;
// re-exported here so `ipc::server::run_server` stays the public entry point.
pub use super::listener::run_server;

#[path = "server_requests.rs"]
mod requests;

#[path = "server_release.rs"]
mod release;

#[cfg(all(test, feature = "gguf"))]
#[path = "server_control_tests.rs"]
mod control_tests;

#[cfg(test)]
#[path = "server_inference_tests.rs"]
mod inference_tests;

#[cfg(test)]
#[path = "server_poison_tests.rs"]
mod poison_tests;

#[cfg(all(test, feature = "gguf"))]
#[path = "server_real_model_tests.rs"]
mod real_model_tests;

#[cfg(test)]
#[path = "server_release_stub.rs"]
mod release_stub;

#[cfg(test)]
#[path = "server_release_daemon.rs"]
mod release_daemon;

#[cfg(test)]
#[path = "server_release_tests.rs"]
mod release_tests;

#[cfg(all(test, feature = "gguf"))]
#[path = "server_release_real_model_tests.rs"]
mod release_real_model_tests;
