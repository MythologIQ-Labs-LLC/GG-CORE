//! Daemon-level head-of-line checks with a real model through the real
//! connection loop and worker (HOL plan Phase 1 + Phase 2).
//!
//! Requires: ../models/qwen2.5-0.5b-instruct-q4_k_m.gguf (skips when absent).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, DuplexStream};

use super::{handle_connection, read_frame};
use crate::engine::gguf::{GgufConfig, GgufGenerator};
use crate::engine::InferenceParams;
use crate::ipc::conn_writer::write_framed;
use crate::ipc::connections::{ConnectionConfig, ConnectionPool, IpcServerConfig};
use crate::ipc::protocol::RequestId;
use crate::ipc::protocol::{decode_message, encode_message, InferenceRequest, IpcMessage};
use crate::memory::ResourceLimitsConfig;
use crate::models::ModelHandle;
use crate::{Runtime, RuntimeConfig};

const TOKEN: &str = "hol-real-model-integration-test-secret";
const MODEL: &str = "../models/qwen2.5-0.5b-instruct-q4_k_m.gguf";
const STALL: Duration = Duration::from_millis(500);

struct Daemon {
    handler: Arc<crate::ipc::IpcHandler>,
    pool: Arc<ConnectionPool>,
    shutdown: tokio_util::sync::CancellationToken,
}

fn load_model() -> Option<GgufGenerator> {
    if !Path::new(MODEL).exists() {
        eprintln!("Skipping: model not found at {MODEL}");
        return None;
    }
    let gguf = GgufConfig {
        n_ctx: 1024,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    Some(GgufGenerator::load("qwen".into(), Path::new(MODEL), &gguf).expect("load"))
}

async fn daemon(max_concurrent: usize) -> Option<Daemon> {
    let model = load_model()?;
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.into(),
        ipc_server: IpcServerConfig {
            stream_stall: STALL,
            ..Default::default()
        },
        resource_limits: ResourceLimitsConfig {
            max_memory_per_call: 1 << 30,
            max_total_memory: 6 << 30,
            max_concurrent,
        },
        ..Default::default()
    });
    let engine = runtime.inference_engine.clone();
    engine
        .register_model("qwen".into(), ModelHandle::new(1), Arc::new(model))
        .await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _worker = crate::scheduler::spawn_worker_with_registry(
        runtime.request_queue.clone(),
        engine,
        None,
        None,
        Some(runtime.resource_limits.clone()),
        Some(runtime.security.clone()),
        shutdown.clone(),
    );
    Some(Daemon {
        handler: Arc::new(runtime.ipc_handler),
        pool: Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 4 })),
        shutdown,
    })
}

/// Open an authenticated connection whose pipe holds `pipe` bytes.
async fn connect(d: &Daemon, pipe: usize) -> (DuplexStream, tokio::task::JoinHandle<()>) {
    let guard = d.pool.try_acquire_owned().expect("slot");
    let (mut client, server) = tokio::io::duplex(pipe);
    let task = tokio::spawn(handle_connection(server, d.handler.clone(), guard, 1 << 20));
    let hello = IpcMessage::Handshake {
        token: TOKEN.into(),
        protocol_version: None,
    };
    send(&mut client, &hello).await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::HandshakeAck { .. }
    ));
    (client, task)
}

async fn send(client: &mut DuplexStream, message: &IpcMessage) {
    let data = encode_message(message).expect("encode");
    write_framed(client, &data).await.expect("send");
}

async fn receive(client: &mut DuplexStream) -> IpcMessage {
    let bytes = tokio::time::timeout(Duration::from_secs(120), read_frame(client, 1 << 20))
        .await
        .expect("frame deadline")
        .expect("frame");
    decode_message(&bytes).expect("decode")
}

fn generate(id: u64, prompt: &str, max_tokens: usize, stream: bool) -> IpcMessage {
    IpcMessage::InferenceRequest(InferenceRequest {
        request_id: RequestId(id),
        model_id: "qwen".into(),
        prompt: prompt.into(),
        parameters: InferenceParams {
            max_tokens,
            stream,
            timeout_ms: Some(120_000),
            ..Default::default()
        },
    })
}

const LONG: &str = "Write the numbers from 1 to 400, one per line.";

/// Phase 1, one worker: a stream whose reader stops cannot hold the only
/// worker; the next client is served and the stalled connection is closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_reader_does_not_starve_the_next_client() {
    let Some(d) = daemon(1).await else { return };
    let (mut stalled, stalled_task) = connect(&d, 1024).await;
    send(&mut stalled, &generate(1, LONG, 400, true)).await;
    tokio::time::sleep(Duration::from_secs(2)).await; // generating; never read

    let (mut other, _other_task) = connect(&d, 64 * 1024).await;
    let start = Instant::now();
    send(
        &mut other,
        &generate(2, "What is 2 + 2? Answer briefly.", 8, false),
    )
    .await;
    let reply = receive(&mut other).await;
    let waited = start.elapsed();
    eprintln!("next client answered after {waited:?} behind a stalled reader");
    match reply {
        IpcMessage::InferenceResponse(r) => assert!(r.error.is_none(), "{r:?}"),
        other => panic!("expected inference_response, got {other:?}"),
    }
    tokio::time::timeout(Duration::from_secs(30), stalled_task)
        .await
        .expect("the stalled connection must be closed by the daemon")
        .expect("joined");
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stalled.read_to_end(&mut rest)).await;
    d.shutdown.cancel();
}

/// Phase 2, two workers: a short request is answered while a healthy long
/// stream is still generating.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn short_request_is_not_serialized_behind_a_long_stream() {
    let Some(d) = daemon(2).await else { return };
    let (mut streaming, _s) = connect(&d, 64 * 1024).await;
    send(&mut streaming, &generate(1, LONG, 400, true)).await;
    // Wait for the first frame so the stream is executing.
    assert!(matches!(
        receive(&mut streaming).await,
        IpcMessage::StreamChunk(_)
    ));

    let (mut other, _o) = connect(&d, 64 * 1024).await;
    send(
        &mut other,
        &generate(2, "What is 2 + 2? Answer briefly.", 8, false),
    )
    .await;
    let reply = tokio::spawn(async move { (receive(&mut other).await, Instant::now()) });

    let mut stream_end = None;
    while stream_end.is_none() {
        if let IpcMessage::StreamChunk(c) = receive(&mut streaming).await {
            if c.is_final {
                assert!(c.error.is_none(), "stream failed: {:?}", c.error);
                stream_end = Some(Instant::now());
            }
        }
    }
    let (reply, answered_at) = reply.await.expect("joined");
    assert!(matches!(reply, IpcMessage::InferenceResponse(ref r) if r.error.is_none()));
    assert!(
        answered_at < stream_end.expect("ended"),
        "the short request waited for the whole stream"
    );
    d.shutdown.cancel();
}
