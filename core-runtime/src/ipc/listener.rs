//! Platform listeners for the IPC server loop.
//!
//! - Unix: a filesystem socket, created owner-only (`0600`) so the IPC
//!   boundary is the daemon's user. A client must still complete the token
//!   handshake; the mode is defense in depth and is set immediately after
//!   `bind` (the umask-governed window before it is sub-millisecond and
//!   still gated by the handshake).
//! - Windows: a named pipe. The first instance is created with
//!   `first_pipe_instance(true)` so an already-existing pipe of the same
//!   name (a squatter impersonating the daemon) fails startup instead of
//!   being silently joined; remote clients are rejected. The default pipe
//!   DACL grants write (and so duplex connect) only to the creator,
//!   Administrators and SYSTEM.

use std::sync::Arc;
#[cfg(windows)]
use std::collections::VecDeque;

use tokio::sync::watch;

use super::connections::{ConnectionPool, IpcServerConfig};
use super::handler::IpcHandler;
use super::server::{spawn_connection, ServerError};

/// Permission bits applied to the Unix socket file after bind.
#[cfg(unix)]
pub(crate) const SOCKET_MODE: u32 = 0o600;

/// Run the IPC server on Unix (Unix domain socket).
#[cfg(unix)]
pub async fn run_server(
    socket_path: String,
    handler: Arc<IpcHandler>,
    connections: Arc<ConnectionPool>,
    mut shutdown_rx: watch::Receiver<bool>,
    ipc_config: IpcServerConfig,
) -> Result<(), ServerError> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path)?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(SOCKET_MODE))?;
    eprintln!(
        "IPC server listening on {} (mode {:o})",
        socket_path, SOCKET_MODE
    );

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

    // Claim the name first (anti-squatting) and keep a bounded pool of
    // unconnected instances available *before* clients arrive. A single
    // waiting instance loses concurrent CreateFile calls with ERROR_PIPE_BUSY
    // during the accept/re-arm gap. Never remove first_pipe_instance(true)
    // from the very first create; every later instance has the same default
    // local-only DACL and reject_remote_clients policy.
    const MAX_PENDING_PIPE_INSTANCES: usize = 16;
    let backlog = connections
        .max_connections()
        .clamp(1, MAX_PENDING_PIPE_INSTANCES);
    let mut pending = VecDeque::with_capacity(backlog);
    pending.push_back(
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&pipe_name)?,
    );
    let new_instance = || {
        ServerOptions::new()
            .first_pipe_instance(false)
            .reject_remote_clients(true)
            .create(&pipe_name)
    };
    for _ in 1..backlog {
        pending.push_back(new_instance()?);
    }
    eprintln!("IPC server listening on {} (pending instances {})", pipe_name, backlog);

    loop {
        // Some clients can have attached to a standby instance already;
        // Tokio connect() handles ERROR_PIPE_CONNECTED in that case.
        let mut server = pending
            .pop_front()
            .expect("Windows named-pipe backlog must remain nonempty");
        tokio::select! {
            result = server.connect() => {
                // Replenish before spawning a handler, including after errors.
                // Standby instances preserve availability during bursts; no
                // client-side retries may bypass auth or the connection cap.
                pending.push_back(new_instance()?);
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
