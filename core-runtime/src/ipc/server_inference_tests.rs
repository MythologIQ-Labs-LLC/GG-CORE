//! Non-streaming requests through the real IPC connection loop: they must be
//! cancellable by `cancel_request` and by disconnect, and bounded by
//! `timeout_ms`, while they wait or run. No worker is spawned, so a request
//! stays pending until its caller stops it; a serial connection loop (the old
//! behavior) would never read the cancel frame and would wait forever.

use super::{handle_connection, read_frame, write_frame};
use crate::engine::InferenceParams;
use crate::ipc::connections::{ConnectionConfig, ConnectionPool};
use crate::ipc::protocol::{
    decode_message, encode_message, InferenceErrorCode, InferenceRequest, IpcMessage, RequestId,
};
use crate::scheduler::RequestQueue;
use crate::{Runtime, RuntimeConfig};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::DuplexStream;

const TOKEN: &str = "nonstream-control-integration-test-secret";

async fn start() -> (DuplexStream, tokio::task::JoinHandle<()>, Arc<RequestQueue>) {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.to_string(),
        ..Default::default()
    });
    let queue = Arc::clone(&runtime.request_queue);
    let handler = Arc::new(runtime.ipc_handler);
    let pool = Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 2 }));
    let guard = pool.try_acquire_owned().expect("slot available");
    let (client, server) = tokio::io::duplex(32 * 1024);
    let task = tokio::spawn(handle_connection(server, handler, guard, 16 * 1024 * 1024));
    (client, task, queue)
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
    let token = TOKEN.to_string();
    send(
        client,
        IpcMessage::Handshake {
            token,
            protocol_version: None,
        },
    )
    .await;
    assert!(matches!(
        receive(client).await,
        IpcMessage::HandshakeAck { .. }
    ));
}

fn request(id: u64, timeout_ms: Option<u64>) -> IpcMessage {
    IpcMessage::InferenceRequest(InferenceRequest {
        request_id: RequestId(id),
        model_id: "waiting-model".into(),
        prompt: "A long non-streaming task that must be cancellable".into(),
        parameters: InferenceParams {
            timeout_ms,
            ..Default::default()
        },
    })
}

fn expect_failure(message: IpcMessage, id: u64, code: InferenceErrorCode) {
    match message {
        IpcMessage::InferenceResponse(r) => {
            assert_eq!(r.request_id.0, id);
            assert_eq!(r.error_code, Some(code), "response: {r:?}");
            assert!(r.error.is_some(), "a stopped request must carry an error");
            assert!(r.output.is_empty() && r.tokens_generated == 0);
        }
        other => panic!("expected inference_response, got {other:?}"),
    }
}

#[tokio::test]
async fn cancel_request_stops_pending_nonstream_request() {
    let (mut client, task, queue) = start().await;
    authenticate(&mut client).await;
    send(&mut client, request(41, Some(60_000))).await;
    send(
        &mut client,
        IpcMessage::CancelRequest {
            request_id: RequestId(41),
        },
    )
    .await;

    let (mut acked, mut failed) = (false, false);
    for _ in 0..2 {
        match receive(&mut client).await {
            IpcMessage::CancelResponse {
                request_id,
                cancelled,
            } => {
                assert_eq!(request_id.0, 41);
                assert!(
                    cancelled,
                    "an active non-streaming request must be cancellable"
                );
                acked = true;
            }
            other => {
                expect_failure(other, 41, InferenceErrorCode::Cancelled);
                failed = true;
            }
        }
    }
    assert!(
        acked && failed,
        "must receive cancel ACK and explicit failure"
    );
    // The queued work was released: the worker would skip it.
    assert!(
        queue.dequeue().await.is_none(),
        "cancelled request must not run"
    );
    drop(client);
    task.await.expect("connection task");
}

#[tokio::test]
async fn nonstream_timeout_is_a_wall_clock_deadline() {
    let (mut client, task, queue) = start().await;
    authenticate(&mut client).await;
    let start = Instant::now();
    send(&mut client, request(42, Some(80))).await;
    expect_failure(
        receive(&mut client).await,
        42,
        InferenceErrorCode::DeadlineExceeded,
    );
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(80),
        "not before the deadline: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "promptly after the deadline: {elapsed:?}"
    );
    assert!(
        queue.dequeue().await.is_none(),
        "expired request must not run"
    );
    drop(client);
    task.await.expect("connection task");
}

#[tokio::test]
async fn disconnect_cancels_nonstream_request() {
    let (mut client, task, queue) = start().await;
    authenticate(&mut client).await;
    send(&mut client, request(43, None)).await;
    // Let the connection enqueue the request before the peer disappears.
    for _ in 0..50 {
        if queue.len().await > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(queue.len().await, 1, "request must be queued");
    drop(client);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("connection task must exit after disconnect")
        .expect("connection task join");
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert!(
        queue.dequeue().await.is_none(),
        "abandoned request must not run"
    );
}

#[tokio::test]
async fn zero_nonstream_timeout_is_invalid_input() {
    let (mut client, task, _queue) = start().await;
    authenticate(&mut client).await;
    send(&mut client, request(44, Some(0))).await;
    expect_failure(
        receive(&mut client).await,
        44,
        InferenceErrorCode::InputInvalid,
    );
    drop(client);
    task.await.expect("connection task");
}

#[tokio::test]
async fn unauthenticated_nonstream_request_is_rejected() {
    let (mut client, task, queue) = start().await;
    send(&mut client, request(45, None)).await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::Error { code: 401, .. }
    ));
    assert_eq!(
        queue.len().await,
        0,
        "unauthenticated work must never be queued"
    );
    drop(client);
    task.await.expect("connection task");
}
