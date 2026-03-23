//! IPC server loop for accepting and processing connections.
//!
//! Uses platform-specific transports:
//! - Unix: `tokio::net::UnixListener` (file-based sockets)
//! - Windows: `tokio::net::windows::named_pipe` (named pipes)
//!
//! All connections use length-prefixed framing (4-byte LE + payload)
//! matching the CLI client protocol in `cli::ipc_client`.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{watch, Mutex};
use tokio_util::sync::CancellationToken;
use thiserror::Error;

use super::connections::{ConnectionPool, IpcServerConfig, OwnedConnectionGuard};
use super::handler::IpcHandler;
use super::protocol::{decode_message, encode_message, IpcMessage};
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

/// Write a length-prefixed frame to an async writer.
async fn write_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    data: &[u8],
) -> Result<(), ServerError> {
    let len = data.len() as u32;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

/// Write a frame using a locked writer.
async fn write_frame_locked<W: AsyncWriteExt + Unpin>(
    writer: &Arc<Mutex<W>>,
    data: &[u8],
) -> Result<(), ServerError> {
    let mut w = writer.lock().await;
    write_frame(&mut *w, data).await
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
    let write_half = Arc::new(Mutex::new(write_half));
    let mut session = None;
    let mut active_streams: HashMap<u64, CancellationToken> = HashMap::new();

    loop {
        let request_bytes = match read_frame(&mut read_half, max_frame_size).await {
            Ok(bytes) => bytes,
            Err(ServerError::Io(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
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
                let _ = write_frame_locked(&write_half, err.as_bytes()).await;
                continue;
            }
        };

        match message {
            // Streaming inference request
            IpcMessage::InferenceRequest(ref req) if req.parameters.stream => {
                if let Some(ref sess) = session {
                    let cancel = CancellationToken::new();
                    active_streams.insert(req.request_id.0, cancel.clone());
                    let bridge = IpcStreamBridge::new(
                        Arc::clone(&write_half),
                        req.request_id,
                        cancel.clone(),
                    );
                    let _ = handler
                        .process_streaming(req.clone(), sess, &bridge, cancel)
                        .await;
                    active_streams.remove(&req.request_id.0);
                } else {
                    let err = r#"{"type":"error","code":401,"message":"Not authenticated"}"#;
                    let _ = write_frame_locked(&write_half, err.as_bytes()).await;
                }
            }

            // Cancel request - trigger cancellation for active streams
            IpcMessage::CancelRequest { request_id } => {
                let cancelled = if let Some(cancel) = active_streams.get(&request_id.0) {
                    cancel.cancel();
                    true
                } else {
                    false
                };
                let response = IpcMessage::CancelResponse { request_id, cancelled };
                if let Ok(bytes) = encode_message(&response) {
                    let _ = write_frame_locked(&write_half, &bytes).await;
                }
            }

            // Non-streaming: use standard request/response processing
            _ => {
                match handler.process(&request_bytes, session.as_ref()).await {
                    Ok((response_bytes, new_session)) => {
                        if new_session.is_some() {
                            session = new_session;
                        }
                        if let Err(e) = write_frame_locked(&write_half, &response_bytes).await {
                            eprintln!("Connection write error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        let err = format!(r#"{{"type":"error","code":500,"message":"{}"}}"#, e);
                        let _ = write_frame_locked(&write_half, err.as_bytes()).await;
                        break;
                    }
                }
            }
        }
    }
}

/// Accept one connection, acquire a guard, and spawn a handler task.
fn spawn_connection<S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static>(
    stream: S,
    handler: &Arc<IpcHandler>,
    connections: &Arc<ConnectionPool>,
    max_frame_size: usize,
) {
    let guard = match connections.try_acquire_owned() {
        Some(g) => g,
        None => {
            eprintln!("Connection limit reached, rejecting client");
            return;
        }
    };
    let handler = Arc::clone(handler);
    tokio::spawn(async move {
        handle_connection(stream, handler, guard, max_frame_size).await;
    });
}

/// Run the IPC server on Unix (Unix domain socket).
#[cfg(unix)]
pub async fn run_server(
    socket_path: String,
    handler: Arc<IpcHandler>,
    connections: Arc<ConnectionPool>,
    mut shutdown_rx: watch::Receiver<bool>,
    ipc_config: IpcServerConfig,
) -> Result<(), ServerError> {
    use tokio::net::UnixListener;

    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path)?;
    eprintln!("IPC server listening on {}", socket_path);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => spawn_connection(
                        stream, &handler, &connections, ipc_config.max_frame_size,
                    ),
                    Err(e) => eprintln!("Accept error: {}", e),
                }
            }
            _ = shutdown_rx.changed() => {
                eprintln!("IPC server shutting down");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

/// Run the IPC server on Windows (named pipes).
#[cfg(windows)]
pub async fn run_server(
    pipe_name: String,
    handler: Arc<IpcHandler>,
    connections: Arc<ConnectionPool>,
    mut shutdown_rx: watch::Receiver<bool>,
    ipc_config: IpcServerConfig,
) -> Result<(), ServerError> {
    use tokio::net::windows::named_pipe::ServerOptions;

    eprintln!("IPC server listening on {}", pipe_name);

    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&pipe_name)?;

        tokio::select! {
            result = server.connect() => {
                match result {
                    Ok(()) => spawn_connection(
                        server, &handler, &connections, ipc_config.max_frame_size,
                    ),
                    Err(e) => eprintln!("Pipe connect error: {}", e),
                }
            }
            _ = shutdown_rx.changed() => {
                eprintln!("IPC server shutting down");
                break;
            }
        }
    }

    Ok(())
}
