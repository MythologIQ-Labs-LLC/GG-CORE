//! Authenticated CLI exchange: connect, handshake, then send one request.
//!
//! The daemon binds a session to the connection that performed the
//! `Handshake`, so every authenticated request (inference, model load /
//! unload, cancel) must travel on a connection that first completed the
//! handshake. The token comes from the `CORE_AUTH_TOKEN` environment
//! variable (the same variable the daemon reads).

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::ipc_client::{CliError, CliIpcClient};
use crate::ipc::protocol::{decode_message, encode_message, IpcMessage};

/// Environment variable carrying the shared IPC auth token.
pub const AUTH_TOKEN_ENV: &str = "CORE_AUTH_TOKEN";

impl CliIpcClient {
    /// Connect, authenticate, then exchange one request/response pair.
    pub async fn send_receive_authenticated(&self, request: &[u8]) -> Result<Vec<u8>, CliError> {
        let mut stream = self.connect_authenticated().await?;
        self.handshake_on(&mut stream).await?;
        self.exchange_data(&mut stream, request).await
    }

    /// Perform the token handshake on an already-connected stream so the
    /// daemon binds a session to it. Fails loud on a rejected token.
    pub(super) async fn handshake_on<S>(&self, stream: &mut S) -> Result<(), CliError>
    where
        S: AsyncReadExt + AsyncWriteExt + Unpin,
    {
        let token = std::env::var(AUTH_TOKEN_ENV).unwrap_or_default();
        let handshake = IpcMessage::Handshake {
            token,
            protocol_version: None,
        };
        let handshake_bytes =
            encode_message(&handshake).map_err(|e| CliError::Protocol(e.to_string()))?;
        let ack_bytes = self.exchange_data(stream, &handshake_bytes).await?;
        let ack = decode_message(&ack_bytes).map_err(|e| CliError::Protocol(e.to_string()))?;
        match ack {
            IpcMessage::HandshakeAck { .. } => Ok(()),
            IpcMessage::Error { message, .. } => Err(CliError::Protocol(format!(
                "authentication failed: {message}"
            ))),
            _ => Err(CliError::Protocol("Unexpected handshake reply".to_string())),
        }
    }

    #[cfg(unix)]
    pub(super) async fn connect_authenticated(&self) -> Result<tokio::net::UnixStream, CliError> {
        use tokio::net::UnixStream;
        use tokio::time::timeout;

        let connect_future = UnixStream::connect(&self.socket_path);
        timeout(self.timeout_duration, connect_future)
            .await
            .map_err(|_| CliError::Timeout)?
            .map_err(|e| CliError::ConnectionFailed(e.to_string()))
    }

    #[cfg(windows)]
    pub(super) async fn connect_authenticated(
        &self,
    ) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, CliError> {
        // Connection retries never retry the handshake, which is bound to
        // this single successfully opened instance and remains fail-closed.
        self.open_windows_pipe().await
    }
}
