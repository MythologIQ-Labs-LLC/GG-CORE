//! Connection poisoning through the real connection loop (HOL plan V1):
//! a client that stops reading makes a frame write stall; the connection is
//! then closed, its in-flight requests are cancelled, and no frame follows
//! the partial one.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, DuplexStream};

use super::{handle_connection, read_frame};
use crate::engine::InferenceParams;
use crate::ipc::conn_writer::write_framed;
use crate::ipc::connections::{ConnectionConfig, ConnectionPool, IpcServerConfig};
use crate::ipc::protocol::{encode_message, InferenceRequest, IpcMessage, RequestId};
use crate::{Runtime, RuntimeConfig};

const TOKEN: &str = "poison-connection-integration-test-secret";
const STALL: Duration = Duration::from_millis(150);

async fn start() -> (DuplexStream, tokio::task::JoinHandle<()>) {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.to_string(),
        ipc_server: IpcServerConfig {
            stream_stall: STALL,
            ..Default::default()
        },
        ..Default::default()
    });
    let handler = Arc::new(runtime.ipc_handler);
    let pool = Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 2 }));
    let guard = pool.try_acquire_owned().expect("slot available");
    // A small pipe: a client that stops reading fills it quickly.
    let (client, server) = tokio::io::duplex(512);
    let task = tokio::spawn(handle_connection(server, handler, guard, 1024 * 1024));
    (client, task)
}

async fn send(client: &mut DuplexStream, message: &IpcMessage) {
    let data = encode_message(message).expect("encode IPC");
    write_framed(client, &data).await.expect("send IPC");
}

async fn authenticate(client: &mut DuplexStream) {
    let handshake = IpcMessage::Handshake {
        token: TOKEN.to_string(),
        protocol_version: None,
    };
    send(client, &handshake).await;
    let ack = tokio::time::timeout(Duration::from_secs(3), read_frame(client, 1 << 20))
        .await
        .expect("handshake deadline")
        .expect("handshake ack");
    assert!(!ack.is_empty());
}

fn pending_request(id: u64) -> IpcMessage {
    IpcMessage::InferenceRequest(InferenceRequest {
        request_id: RequestId(id),
        model_id: "waiting-model".into(),
        prompt: "stays queued: no worker runs in this test".into(),
        parameters: InferenceParams {
            timeout_ms: Some(60_000),
            ..Default::default()
        },
    })
}

/// Complete frames in `bytes`, and whether a truncated frame trails them.
fn whole_frames(bytes: &[u8]) -> (usize, bool) {
    let (mut at, mut count) = (0, 0);
    while bytes.len() >= at + 4 {
        let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if bytes.len() < at + 4 + len {
            return (count, true);
        }
        at += 4 + len;
        count += 1;
    }
    (count, at != bytes.len())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_frame_write_closes_the_connection_and_cancels_its_requests() {
    let (mut client, task) = start().await;
    authenticate(&mut client).await;
    send(&mut client, &pending_request(7)).await;
    // Stop reading and provoke error frames until the pipe is full.
    for _ in 0..64 {
        if write_framed(&mut client, b"not json").await.is_err() {
            break;
        }
    }

    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("a stalled write must end the connection loop")
        .expect("connection task joined");

    // EOF arrives only once every writer reference is gone, i.e. once the
    // pending request (no worker, 60 s timeout) was cancelled.
    let mut rest = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut rest))
        .await
        .expect("the connection must close")
        .expect("drain");
    let (frames, _truncated_tail) = whole_frames(&rest);
    assert!(frames > 0, "error frames were written before the stall");
    assert!(
        !String::from_utf8_lossy(&rest).contains("inference_response"),
        "no response may follow the stalled frame"
    );
}
