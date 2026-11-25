//! CLI models list authentication ordering for issue #171.
//! Fake local sockets do not require real models or alter process auth env.

use super::{CliError, CliIpcClient};
use crate::ipc::protocol::{
    decode_message, encode_message, IpcMessage, ModelInfo, ModelsListResponse, ProtocolVersion,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::{timeout, Duration};

async fn read_message(stream: &mut UnixStream) -> IpcMessage {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await.expect("read frame header");
    let len = u32::from_le_bytes(header) as usize;
    assert!((1..65536).contains(&len), "bounded client frame");
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await.expect("read frame body");
    decode_message(&payload).expect("valid IPC message")
}

async fn send_message(stream: &mut UnixStream, message: &IpcMessage) {
    let payload = encode_message(message).expect("encode server response");
    stream.write_all(&(payload.len() as u32).to_le_bytes())
        .await.expect("write frame header");
    stream.write_all(&payload).await.expect("write frame body");
}

fn socket() -> (tempfile::TempDir, UnixListener, String) {
    let dir = tempfile::tempdir().expect("isolated socket directory");
    let path = dir.path().join("models.sock");
    let listener = UnixListener::bind(&path).expect("bind local socket");
    (dir, listener, path.to_string_lossy().into_owned())
}

#[tokio::test]
async fn models_list_handshakes_before_request_and_receives_capabilities() {
    let (_dir, listener, path) = socket();
    let server = tokio::spawn(async move {
        let (mut conn, _) = listener.accept().await.expect("accept client");
        let hello = read_message(&mut conn).await;
        assert!(matches!(hello, IpcMessage::Handshake { .. }),
                "models list must authenticate first: {hello:?}");
        send_message(&mut conn, &IpcMessage::HandshakeAck {
            session_id: "models-list-test-session".into(),
            protocol_version: ProtocolVersion::V1,
            features: vec![],
        }).await;
        assert!(matches!(read_message(&mut conn).await, IpcMessage::ModelsRequest));
        let reply = ModelsListResponse {
            total_memory_bytes: 256,
            models: vec![ModelInfo {
                handle_id: 5,
                name: "onnx-embeddings".into(),
                format: "onnx".into(),
                size_bytes: 256,
                memory_bytes: 256,
                state: "ready".into(),
                request_count: 0,
                avg_latency_ms: 0.0,
                loaded_at: "2026-11-25T00:00:00Z".into(),
                capabilities: vec!["embedding".into()],
            }],
        };
        send_message(&mut conn, &IpcMessage::ModelsResponse(reply)).await;
    });
    let client = CliIpcClient::new(path);
    let response = timeout(Duration::from_secs(3), client.get_models())
        .await.expect("models list must finish")
        .expect("authenticated model listing");
    assert_eq!(response.models.len(), 1);
    assert_eq!(response.models[0].capabilities, vec!["embedding".to_string()]);
    server.await.expect("mock daemon assertion");
}

#[tokio::test]
async fn auth_refusal_must_not_retry_unauthenticated_models_request() {
    let (_dir, listener, path) = socket();
    let server = tokio::spawn(async move {
        let (mut conn, _) = listener.accept().await.expect("accept client");
        assert!(matches!(read_message(&mut conn).await, IpcMessage::Handshake { .. }));
        send_message(&mut conn, &IpcMessage::Error {
            code: 401,
            message: "invalid authentication".into(),
        }).await;
        let mut byte = [0u8; 1];
        let read = timeout(Duration::from_secs(3), conn.read(&mut byte))
            .await.expect("client must close after refusal")
            .expect("read after rejection");
        assert_eq!(read, 0, "no unauthenticated fallback or model request");
    });
    let client = CliIpcClient::new(path);
    let result = timeout(Duration::from_secs(3), client.get_models())
        .await.expect("auth refusal must finish");
    assert!(matches!(result, Err(CliError::Protocol(msg)) if msg.contains("authentication failed")));
    server.await.expect("mock daemon assertion");
}

#[tokio::test]
async fn models_list_cli_maps_auth_refusal_to_failure_not_connection_loss() {
    let (_dir, listener, path) = socket();
    let server = tokio::spawn(async move {
        let (mut conn, _) = listener.accept().await.expect("accept client");
        assert!(matches!(read_message(&mut conn).await, IpcMessage::Handshake { .. }));
        send_message(&mut conn, &IpcMessage::Error {
            code: 401,
            message: "invalid authentication".into(),
        }).await;
    });
    let code = timeout(Duration::from_secs(3), crate::cli::models_cmd::run_list(&path, true))
        .await.expect("CLI auth refusal must finish");
    assert_eq!(code, 1, "authentication failure is not a transport outage");
    server.await.expect("mock daemon assertion");
}
