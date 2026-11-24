//! Unix-socket integration tests for the IPC trust boundary (RC acceptance).
//!
//! Spins up the real server loop *and* the scheduler worker (exactly what
//! `gg-core-cli serve` runs) on a temp socket and drives it with the shipped
//! CLI client, proving:
//! - the socket file is created owner-only (0600);
//! - unauthenticated probes (health) still work;
//! - CLI inference (non-streaming and streaming) performs the token
//!   handshake on its connection — a wrong token fails closed, the right
//!   token gets past authentication.
//!
//! The worker matters: under the `gguf` feature an inference request is
//! enqueued and answered by the worker; without one the request never
//! completes and the client waits forever. Every client call is bounded so a
//! regression fails instead of hanging CI.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use gg_core::cli::ipc_client_auth::AUTH_TOKEN_ENV;
use gg_core::cli::{CliError, CliIpcClient};
use gg_core::engine::InferenceParams;
use gg_core::ipc::{ConnectionConfig, ConnectionPool, HealthCheckType, IpcServerConfig};
use gg_core::{Runtime, RuntimeConfig};

const TOKEN: &str = "unix-socket-test-token";
const CALL_BUDGET: Duration = Duration::from_secs(30);

// CORE_AUTH_TOKEN is process-global; serialize the tests that set it
// (async-aware lock: it is held across awaits by design).
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Server {
    socket: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<(), gg_core::ipc::server::ServerError>>,
    worker_shutdown: tokio_util::sync::CancellationToken,
    worker: tokio::task::JoinHandle<()>,
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

        // Same worker wiring as runtime_init::run_ipc_server.
        let worker_shutdown = tokio_util::sync::CancellationToken::new();
        let worker = gg_core::scheduler::spawn_worker_with_registry(
            runtime.request_queue.clone(),
            runtime.inference_engine.clone(),
            Some(runtime.model_lifecycle.clone()),
            Some(runtime.model_registry.clone()),
            Some(runtime.resource_limits.clone()),
            Some(runtime.security.clone()),
            worker_shutdown.clone(),
        );

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
            worker_shutdown,
            worker,
            _dir: dir,
        }
    }

    fn client(&self) -> CliIpcClient {
        CliIpcClient::new(self.socket.clone()).with_timeout(Duration::from_secs(5))
    }

    /// Bounded CLI inference; a hang is a test failure, never a CI stall.
    async fn infer(&self, streaming: bool, model: &str) -> Result<String, CliError> {
        let client = self.client();
        let params = InferenceParams::default();
        let call = async {
            if streaming {
                client.send_streaming_inference(model, "hi", &params).await
            } else {
                client.send_inference(model, "hi", &params).await
            }
        };
        tokio::time::timeout(CALL_BUDGET, call)
            .await
            .unwrap_or_else(|_| {
                panic!("CLI inference (streaming={streaming}) hung past {CALL_BUDGET:?}")
            })
    }

    async fn stop(self) {
        let _ = self.shutdown.send(true);
        self.worker_shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.task).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), self.worker).await;
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
    let alive = tokio::time::timeout(
        CALL_BUDGET,
        server.client().check_health(HealthCheckType::Liveness),
    )
    .await
    .expect("liveness probe hung");
    assert!(alive.unwrap());
    server.stop().await;
}

#[tokio::test]
async fn cli_inference_with_wrong_token_fails_closed() {
    let _guard = ENV_LOCK.lock().await;
    let server = Server::start().await;
    std::env::set_var(AUTH_TOKEN_ENV, "not-the-token");
    for streaming in [false, true] {
        let err = server
            .infer(streaming, "any")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("authentication failed"),
            "streaming={streaming}: got: {err}"
        );
    }
    std::env::remove_var(AUTH_TOKEN_ENV);
    server.stop().await;
}

#[tokio::test]
async fn cli_inference_with_correct_token_passes_authentication() {
    let _guard = ENV_LOCK.lock().await;
    let server = Server::start().await;
    std::env::set_var(AUTH_TOKEN_ENV, TOKEN);
    // No model is loaded, so the request fails *after* authentication:
    // the error must be about the model, never about the session.
    for streaming in [false, true] {
        let err = server
            .infer(streaming, "no-such-model")
            .await
            .unwrap_err()
            .to_string();
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
    let err = server.infer(false, "any").await.unwrap_err().to_string();
    assert!(err.contains("authentication failed"), "got: {err}");
    server.stop().await;
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn daemon_starts_under_seccomp_with_private_unix_socket() {
    use std::process::{Child, Command, Stdio};

    struct Reap(Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // Unlike the in-process server tests, this runs the real serve bootstrap:
    // model-free initialization, sandbox install, Unix bind and IPC liveness.
    // A missing SYS_geteuid allowlist entry would kill this child at bind.
    let dir = tempfile::tempdir().unwrap();
    let socket = dir
        .path()
        .join("seccomp.sock")
        .to_string_lossy()
        .into_owned();
    let mut daemon = Reap(
        Command::new(env!("CARGO_BIN_EXE_gg-core-cli"))
            .arg("serve")
            .env("CORE_AUTH_TOKEN", TOKEN)
            .env("GG_CORE_SANDBOX", "seccomp")
            .env("GG_CORE_SOCKET_PATH", &socket)
            .env_remove("GG_CORE_PRELOAD_MODELS")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("launch real sandboxed daemon"),
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(exit) = daemon.0.try_wait().expect("check daemon exit") {
            panic!("sandboxed daemon exited before IPC liveness was usable: {exit}");
        }
        if std::path::Path::new(&socket).exists() {
            let client = CliIpcClient::new(socket.clone()).with_timeout(Duration::from_millis(500));
            if matches!(
                client.check_health(HealthCheckType::Liveness).await,
                Ok(true)
            ) {
                assert!(daemon.0.try_wait().unwrap().is_none());
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "default Linux seccomp daemon did not serve IPC within 15 seconds"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn operator_verify_requires_authenticated_offline_ipc_and_refuses_unready_model() {
    let _guard = ENV_LOCK.lock().await;
    let server = Server::start().await;
    let args = vec!["--socket".to_string(), server.socket.clone()];
    std::env::set_var(AUTH_TOKEN_ENV, TOKEN);
    assert_eq!(
        gg_core::cli::verify_cmd::run_verify(&args).await,
        0,
        "a running model-free daemon has valid IPC liveness and authenticated capabilities"
    );
    let all = vec![
        "--all".to_string(),
        "--socket".to_string(),
        server.socket.clone(),
    ];
    assert_eq!(
        gg_core::cli::verify_cmd::run_verify(&all).await,
        1,
        "--all must not claim inference-readiness without a loaded model"
    );
    std::env::set_var(AUTH_TOKEN_ENV, "wrong-token");
    assert_eq!(
        gg_core::cli::verify_cmd::run_verify(&args).await,
        1,
        "a healthy unauthenticated probe must not mask invalid credentials"
    );
    std::env::remove_var(AUTH_TOKEN_ENV);
    assert_eq!(
        gg_core::cli::verify_cmd::run_verify(&args).await,
        1,
        "a missing token must never produce a verification PASS"
    );
    server.stop().await;
}

#[tokio::test]
async fn shipped_verify_subcommand_routes_through_authenticated_ipc() {
    use tokio::process::Command;

    let server = Server::start().await;
    let socket = server.socket.as_str();
    let cases: &[(&[&str], &str, i32)] = &[
        (&["verify", "--socket", socket], TOKEN, 0),
        (&["verify", "--all", "--socket", socket], TOKEN, 1),
        (&["verify", "--socket", socket], "invalid-token", 1),
        (&["verify", "--unknown"], TOKEN, 2),
    ];
    for (args, token, expected) in cases {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gg-core-cli"));
        command.args(*args).env(AUTH_TOKEN_ENV, token).kill_on_drop(true);
        let child = tokio::time::timeout(Duration::from_secs(15), command.output())
            .await
            .expect("shipped verify command exceeded bounded deadline")
            .expect("failed to launch packaged-style CLI executable");
        assert_eq!(
            child.status.code(),
            Some(*expected),
            "CLI arguments {args:?} must have the expected safe exit status"
        );
        let stderr = String::from_utf8_lossy(&child.stderr);
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(
            !stdout.contains(token) && !stderr.contains(token),
            "verify must not expose authentication credentials"
        );
    }
    server.stop().await;
}
