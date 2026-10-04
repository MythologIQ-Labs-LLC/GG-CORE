//! Authenticated single-connection IPC exchange for CLI commands (B-41, #106).
//!
//! The server binds a session to the connection that performed the
//! `Handshake`, so an authenticated command must handshake and send its
//! request on the same stream. The shared auth token comes from the
//! `CORE_AUTH_TOKEN` environment variable (the same variable the daemon
//! reads); unset means an empty token, which only succeeds against a daemon
//! configured with an empty token.

use super::ipc_client::{CliError, CliIpcClient};
use crate::ipc::protocol::{decode_message, encode_message, IpcMessage};

/// Environment variable holding the shared IPC auth token.
pub const AUTH_TOKEN_ENV: &str = "CORE_AUTH_TOKEN";

impl CliIpcClient {
    /// Send one request on an authenticated connection and return the
    /// response bytes: connect, `Handshake` (token from `CORE_AUTH_TOKEN`),
    /// verify the ack, then exchange the request on the same stream.
    pub async fn send_receive_authenticated(&self, request: &[u8]) -> Result<Vec<u8>, CliError> {
        let token = std::env::var(AUTH_TOKEN_ENV).unwrap_or_default();
        let handshake = IpcMessage::Handshake {
            token,
            protocol_version: None,
        };
        let handshake_bytes =
            encode_message(&handshake).map_err(|e| CliError::Protocol(e.to_string()))?;

        let mut stream = self.connect_authenticated().await?;
        let ack_bytes = self.exchange_data(&mut stream, &handshake_bytes).await?;
        let ack = decode_message(&ack_bytes).map_err(|e| CliError::Protocol(e.to_string()))?;
        match ack {
            IpcMessage::HandshakeAck { .. } => {}
            IpcMessage::Error { message, .. } => {
                return Err(CliError::Protocol(format!(
                    "authentication failed: {message}"
                )))
            }
            _ => return Err(CliError::Protocol("Unexpected handshake reply".to_string())),
        }

        self.exchange_data(&mut stream, request).await
    }

    #[cfg(unix)]
    async fn connect_authenticated(&self) -> Result<tokio::net::UnixStream, CliError> {
        use tokio::net::UnixStream;
        use tokio::time::timeout;

        let connect_future = UnixStream::connect(&self.socket_path);
        timeout(self.timeout_duration, connect_future)
            .await
            .map_err(|_| CliError::Timeout)?
            .map_err(|e| CliError::ConnectionFailed(e.to_string()))
    }

    #[cfg(windows)]
    async fn connect_authenticated(
        &self,
    ) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, CliError> {
        use tokio::net::windows::named_pipe::ClientOptions;
        use tokio::time::timeout;

        let connect_future = ClientOptions::new().open(&self.socket_path);
        timeout(self.timeout_duration, async { connect_future })
            .await
            .map_err(|_| CliError::Timeout)?
            .map_err(|e| CliError::ConnectionFailed(e.to_string()))
    }
}
