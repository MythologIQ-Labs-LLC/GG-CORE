//! Unix-socket integration tests for the IPC trust boundary (RC acceptance).
//!
//! Spins up the real server loop on a temp socket and drives it with the
//! shipped CLI client, proving:
//! - the socket file is created owner-only (0600);
//! - unauthenticated probes (health) still work;
//! - CLI inference (non-streaming and streaming) performs the token
//!   handshake on its connection — a wrong token fails closed, the right
//!   token gets past authentication.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use gg_core::cli::ipc_client_auth::AUTH_TOKEN_ENV;
use gg_core::cli::CliIpcClient;
use gg_core::engine::InferenceParams;
use gg_core::ipc::{ConnectionConfig, ConnectionPool, HealthCheckType, IpcServerConfig};
use gg_core::{Runtime, RuntimeConfig};

const TOKEN: &str = "unix-socket-test-token";

// CORE_AUTH_TOKEN is process-global; serialize the tests that set it
// (async-aware lock: it is held across awaits by design).
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Server {
    socket: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<(), gg_core::ipc::server::ServerError>>,
    _dir: tempfile::TempDir,
}

impl Server {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("gg.sock").to_string_lossy().into_owned();
        let runtime = Runtime::new(RuntimeConfig {
            auth_token: TOKEN.into(),
            ..Default::default()
        });
        let handler = Arc::new(runtime.ipc_handler);
        let pool = Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 4 }));
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let path = socket.clone();
        let task = tokio::spawn(async move {
            gg_core::ipc::server::run_server(path, handler, pool, rx, IpcServerConfig::default())
                .await
        });
        for _ in 0..50 {
            if std::path::Path::new(&socket).exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(std::path::Path::new(&socket).exists(), "server never bound");
        Self {
            socket,
            shutdown,
            task,
            _dir: dir,
        }
    }

    fn client(&self) -> CliIpcClient {
        CliIpcClient::new(self.socket.clone()).with_timeout(Duration::from_secs(5))
    }

    async fn stop(self) {
        let _ = self.shutdown.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.task).await;
    }
}

#[tokio::test]
async fn socket_file_is_owner_only() {
    let server = Server::start().await;
    let mode = std::fs::metadata(&server.socket)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "socket mode {mode:o}");
    server.stop().await;
}

#[tokio::test]
async fn unauthenticated_liveness_probe_works() {
    let server = Server::start().await;
    assert!(server
        .client()
        .check_health(HealthCheckType::Liveness)
        .await
        .unwrap());
    server.stop().await;
}

#[tokio::test]
async fn cli_inference_with_wrong_token_fails_closed() {
    let _guard = ENV_LOCK.lock().await;
    let server = Server::start().await;
    std::env::set_var(AUTH_TOKEN_ENV, "not-the-token");
    let params = InferenceParams::default();
    let err = server
        .client()
        .send_inference("any", "hi", &params)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("authentication failed"), "got: {err}");
    let err = server
        .client()
        .send_streaming_inference("any", "hi", &params)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("authentication failed"), "got: {err}");
    std::env::remove_var(AUTH_TOKEN_ENV);
    server.stop().await;
}

#[tokio::test]
async fn cli_inference_with_correct_token_passes_authentication() {
    let _guard = ENV_LOCK.lock().await;
    let server = Server::start().await;
    std::env::set_var(AUTH_TOKEN_ENV, TOKEN);
    let params = InferenceParams::default();
    // No model is loaded, so the request fails *after* authentication:
    // the error must be about the model, never about the session.
    for streaming in [false, true] {
        let client = server.client();
        let result = if streaming {
            client
                .send_streaming_inference("no-such-model", "hi", &params)
                .await
        } else {
            client.send_inference("no-such-model", "hi", &params).await
        };
        let err = result.unwrap_err().to_string();
        assert!(
            !err.to_lowercase().contains("authenticat"),
            "streaming={streaming}: authentication must have succeeded, got: {err}"
        );
    }
    std::env::remove_var(AUTH_TOKEN_ENV);
    server.stop().await;
}

#[tokio::test]
async fn empty_client_token_is_rejected_by_a_token_protected_daemon() {
    let _guard = ENV_LOCK.lock().await;
    let server = Server::start().await;
    std::env::remove_var(AUTH_TOKEN_ENV);
    let err = server
        .client()
        .send_inference("any", "hi", &InferenceParams::default())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("authentication failed"), "got: {err}");
    server.stop().await;
}
