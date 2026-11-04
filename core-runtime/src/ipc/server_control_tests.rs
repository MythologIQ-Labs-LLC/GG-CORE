//! Tests the actual IPC connection loop, not only the cancellation flag.
//! No model or worker is needed: a pending streaming request must remain
//! cancellable and deadline-bounded while its queue is idle.

use super::{handle_connection, read_frame, write_frame};
use crate::engine::InferenceParams;
use crate::ipc::connections::{ConnectionConfig, ConnectionPool};
use crate::ipc::protocol::{
    decode_message, encode_message, HealthCheckType, InferenceRequest, IpcMessage, RequestId,
    WarmupRequest,
};
use crate::{Runtime, RuntimeConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::DuplexStream;

const TOKEN: &str = "stream-control-integration-test-secret";

async fn start() -> (DuplexStream, tokio::task::JoinHandle<()>) {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.to_string(),
        ..Default::default()
    });
    let handler = Arc::new(runtime.ipc_handler);
    let pool = Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 2 }));
    let guard = pool.try_acquire_owned().expect("slot available");
    let (client, server) = tokio::io::duplex(32 * 1024);
    let task = tokio::spawn(handle_connection(server, handler, guard, 16 * 1024 * 1024));
    (client, task)
}

async fn send(client: &mut DuplexStream, message: IpcMessage) {
    let data = encode_message(&message).expect("encode IPC");
    write_frame(client, &data).await.expect("send IPC");
}

async fn receive(client: &mut DuplexStream) -> IpcMessage {
    let bytes = tokio::time::timeout(Duration::from_secs(3), read_frame(client, 16 * 1024 * 1024))
        .await
        .expect("IPC response deadline")
        .expect("read response");
    decode_message(&bytes).expect("decode response")
}

async fn authenticate(client: &mut DuplexStream) {
    send(
        client,
        IpcMessage::Handshake {
            token: TOKEN.to_string(),
            protocol_version: None,
        },
    )
    .await;
    assert!(matches!(
        receive(client).await,
        IpcMessage::HandshakeAck { .. }
    ));
}

fn streaming_request(timeout_ms: u64) -> IpcMessage {
    let params = InferenceParams {
        stream: true,
        timeout_ms: Some(timeout_ms),
        ..Default::default()
    };
    IpcMessage::InferenceRequest(InferenceRequest {
        request_id: RequestId(73),
        model_id: "waiting-model".into(),
        prompt: "A long task that must be cancellable".into(),
        parameters: params,
    })
}

async fn close(client: DuplexStream, task: tokio::task::JoinHandle<()>) {
    drop(client);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("connection task must exit after peer disconnect")
        .expect("connection task join");
}

#[tokio::test]
async fn authenticated_cancel_is_processed_while_stream_is_waiting() {
    let (mut client, task) = start().await;
    authenticate(&mut client).await;

    // There is intentionally no worker: a serial server loop would hang on
    // the stream and never process this cancellation.
    send(&mut client, streaming_request(60_000)).await;
    send(
        &mut client,
        IpcMessage::CancelRequest {
            request_id: RequestId(73),
        },
    )
    .await;

    let mut acknowledged = false;
    let mut terminal = false;
    for _ in 0..3 {
        match receive(&mut client).await {
            IpcMessage::CancelResponse {
                request_id,
                cancelled,
            } => {
                assert_eq!(request_id.0, 73);
                assert!(cancelled, "active stream must be cancellable");
                acknowledged = true;
            }
            IpcMessage::StreamChunk(chunk) if chunk.is_final => {
                assert!(chunk
                    .error
                    .as_deref()
                    .is_some_and(|e| e.contains("cancelled")));
                terminal = true;
            }
            other => panic!("unexpected IPC response: {other:?}"),
        }
        if acknowledged && terminal {
            break;
        }
    }
    assert!(
        acknowledged && terminal,
        "must receive cancel ACK and terminal"
    );
    close(client, task).await;
}

#[tokio::test]
async fn idle_stream_has_real_wall_clock_deadline() {
    let (mut client, task) = start().await;
    authenticate(&mut client).await;
    send(&mut client, streaming_request(50)).await;
    match receive(&mut client).await {
        IpcMessage::StreamChunk(chunk) => {
            assert!(chunk.is_final);
            assert!(chunk
                .error
                .as_deref()
                .is_some_and(|e| e.contains("deadline")));
        }
        other => panic!("expected timeout terminal: {other:?}"),
    }
    close(client, task).await;
}

#[tokio::test]
async fn zero_timeout_is_a_terminal_failure_not_a_hang() {
    let (mut client, task) = start().await;
    authenticate(&mut client).await;
    send(&mut client, streaming_request(0)).await;
    match receive(&mut client).await {
        IpcMessage::StreamChunk(chunk) => {
            assert!(chunk.is_final);
            assert!(chunk.error.as_deref().is_some_and(|e| e.contains("timeout")));
        }
        other => panic!("expected invalid-timeout terminal: {other:?}"),
    }
    close(client, task).await;
}

#[tokio::test]
async fn unauthenticated_liveness_probe_remains_available() {
    let (mut client, task) = start().await;
    send(
        &mut client,
        IpcMessage::HealthCheck {
            check_type: HealthCheckType::Liveness,
        },
    )
    .await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::HealthResponse(_)
    ));
    close(client, task).await;
}

#[tokio::test]
async fn unauthenticated_warmup_cannot_invoke_inference() {
    let (mut client, task) = start().await;
    send(
        &mut client,
        IpcMessage::WarmupRequest(WarmupRequest {
            model_id: "waiting-model".into(),
            tokens: 1,
        }),
    )
    .await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::Error { code: 401, .. }
    ));
    close(client, task).await;
}

#[tokio::test]
async fn unauthenticated_cancel_is_rejected() {
    let (mut client, task) = start().await;
    send(
        &mut client,
        IpcMessage::CancelRequest {
            request_id: RequestId(73),
        },
    )
    .await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::Error { code: 401, .. }
    ));
    close(client, task).await;
}
