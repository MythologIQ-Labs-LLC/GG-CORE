//! Real-model release of an unloaded instance through the real connection
//! loop and worker (MODEL_RELEASE_V1_PLAN A10, R4, S3).
//!
//! Requires ../models/qwen2.5-0.5b-instruct-q4_k_m.gguf. Skips when only a
//! Git LFS pointer is present, but panics when GG_CORE_REQUIRE_REAL_MODEL=1.
//! With GG_CORE_RELEASE_BUDGET_OUT set, writes {R, W, budget, n_ctx} JSON.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::DuplexStream;

use super::release::TEST_LOCK;
use super::{handle_connection, read_frame};
use crate::engine::gguf::{GgufConfig, GgufGenerator};
use crate::engine::{InferenceParams, Model};
use crate::ipc::conn_writer::write_framed;
use crate::ipc::connections::{ConnectionConfig, ConnectionPool, IpcServerConfig};
use crate::ipc::protocol::*;
use crate::memory::ResourceLimitsConfig;
use crate::models::{LifecycleError, ModelLifecycle, ModelMetadata};
use crate::{Runtime, RuntimeConfig};

const TOKEN: &str = "release-real-model-integration-test-secret";
const MODEL: &str = "../models/qwen2.5-0.5b-instruct-q4_k_m.gguf";
const N_CTX: u32 = 512;
const LONG: &str = "Write the numbers from 1 to 400, one per line.";

fn instance() -> Option<Arc<dyn Model>> {
    let mut magic = [0u8; 4];
    let is_gguf = std::fs::File::open(MODEL)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .is_ok()
        && &magic == b"GGUF";
    if !is_gguf {
        let required = std::env::var("GG_CORE_REQUIRE_REAL_MODEL").as_deref() == Ok("1");
        assert!(
            !required,
            "GG_CORE_REQUIRE_REAL_MODEL=1 but {MODEL} is not a GGUF file"
        );
        eprintln!("Skipping: real GGUF model not present at {MODEL}");
        return None;
    }
    let config = GgufConfig {
        n_ctx: N_CTX,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    let generator = GgufGenerator::load("qwen".into(), Path::new(MODEL), &config).expect("load");
    Some(Arc::new(generator))
}

/// `budget = R + W + (R - W) / 2`: one instance plus one request fits, two
/// instances do not (plan §10/§11 arithmetic).
fn budget(model: &dyn Model) -> (usize, usize, usize) {
    let r = model.memory_usage();
    let w = model
        .working_memory_estimate()
        .expect("GGUF working estimate");
    assert!(w < r, "arithmetic precondition W < R failed: R={r} W={w}");
    let budget = r + w + (r - w) / 2;
    if let Ok(path) = std::env::var("GG_CORE_RELEASE_BUDGET_OUT") {
        let json = format!(r#"{{"R":{r},"W":{w},"budget":{budget},"n_ctx":{N_CTX}}}"#);
        std::fs::write(path, json).expect("write budget");
    }
    eprintln!("release budget: R={r} W={w} budget={budget}");
    (r, w, budget)
}

struct Daemon {
    handler: Arc<crate::ipc::IpcHandler>,
    pool: Arc<ConnectionPool>,
    lifecycle: Arc<ModelLifecycle>,
    shutdown: tokio_util::sync::CancellationToken,
}

async fn daemon(a: Arc<dyn Model>, w: usize, total: usize) -> Daemon {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.into(),
        ipc_server: IpcServerConfig {
            stream_stall: Duration::from_secs(60),
            ..Default::default()
        },
        resource_limits: ResourceLimitsConfig {
            max_memory_per_call: w + (1 << 20),
            max_total_memory: total,
            max_concurrent: 2,
        },
        ..Default::default()
    });
    let lifecycle = Arc::clone(&runtime.model_lifecycle);
    lifecycle.load("a".into(), meta(), a).await.expect("a fits");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _worker = crate::scheduler::spawn_worker_with_registry(
        runtime.request_queue.clone(),
        runtime.inference_engine.clone(),
        None,
        None,
        Some(runtime.resource_limits.clone()),
        Some(runtime.security.clone()),
        shutdown.clone(),
    );
    Daemon {
        handler: Arc::new(runtime.ipc_handler),
        pool: Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 4 })),
        lifecycle,
        shutdown,
    }
}

fn meta() -> ModelMetadata {
    ModelMetadata {
        name: "qwen2.5-0.5b".into(),
        size_bytes: 0,
    }
}

async fn connect(d: &Daemon, pipe: usize) -> DuplexStream {
    let guard = d.pool.try_acquire_owned().expect("slot");
    let (mut client, server) = tokio::io::duplex(pipe);
    tokio::spawn(handle_connection(server, d.handler.clone(), guard, 1 << 20));
    let hello = IpcMessage::Handshake {
        token: TOKEN.into(),
        protocol_version: None,
    };
    send(&mut client, &hello).await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::HandshakeAck { .. }
    ));
    client
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

fn generate(id: u64, model: &str, max_tokens: usize, stream: bool) -> IpcMessage {
    let prompt = if stream {
        LONG
    } else {
        "What is 2 + 2? Answer briefly."
    };
    IpcMessage::InferenceRequest(InferenceRequest {
        request_id: RequestId(id),
        model_id: model.into(),
        prompt: prompt.into(),
        parameters: InferenceParams {
            max_tokens,
            stream,
            timeout_ms: Some(120_000),
            ..Default::default()
        },
    })
}

async fn release_wait(c: &mut DuplexStream, ms: u64) -> ModelReleaseWaitResponse {
    let wait = ModelReleaseWaitRequest::new("a", ms);
    send(c, &IpcMessage::ModelReleaseWaitRequest(wait)).await;
    match receive(c).await {
        IpcMessage::ModelReleaseWaitResponse(w) => w,
        other => panic!("expected model_release_wait_response, got {other:?}"),
    }
}

async fn infer_reply(c: &mut DuplexStream, id: u64, model: &str) -> InferenceResponse {
    send(c, &generate(id, model, 8, false)).await;
    match receive(c).await {
        IpcMessage::InferenceResponse(r) => r,
        other => panic!("expected inference_response, got {other:?}"),
    }
}

/// Read the held stream to its end: exactly one final frame, no error.
async fn drain_stream(stream: &mut DuplexStream) {
    loop {
        if let IpcMessage::StreamChunk(chunk) = receive(stream).await {
            if chunk.is_final {
                assert!(chunk.error.is_none(), "stream failed: {:?}", chunk.error);
                return;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_instance_is_released_only_after_its_stream_ends() {
    let _l = TEST_LOCK.lock().await;
    let Some(a) = instance() else { return };
    let (_r, w, total) = budget(a.as_ref());
    let d = daemon(a, w, total).await;

    // 1. A stream on `a` whose reader stops: back-pressure holds the model.
    let mut stream = connect(&d, 1024).await;
    send(&mut stream, &generate(1, "a", 400, true)).await;
    assert!(matches!(
        receive(&mut stream).await,
        IpcMessage::StreamChunk(_)
    ));

    // 2-4. Unload succeeds; the held instance is listed once as unloading.
    let mut c = connect(&d, 64 * 1024).await;
    let unload = ModelUnloadRequest {
        model_id: "a".into(),
    };
    send(&mut c, &IpcMessage::ModelUnloadRequest(unload)).await;
    let reply = receive(&mut c).await;
    assert!(
        matches!(reply, IpcMessage::ModelUnloadResponse(ref r) if r.success),
        "{reply:?}"
    );
    assert_eq!(d.lifecycle.releasing_models().len(), 1);

    // 5. A second instance does not fit while `a` is still held.
    let b = instance().expect("same fixture");
    let refused = d.lifecycle.load("b".into(), meta(), b).await;
    assert!(
        matches!(refused, Err(LifecycleError::ResourceExhausted(_))),
        "{refused:?}"
    );

    // 6. Not released while held.
    let held = release_wait(&mut c, 2_000).await;
    assert!(!held.released && held.pending_instances == 1, "{held:?}");

    // 7-8. Resume reading: the stream ends cleanly on `a`, then `a` is released.
    drain_stream(&mut stream).await;
    let done = release_wait(&mut c, 30_000).await;
    assert!(done.released && done.pending_instances == 0, "{done:?}");
    assert!(d.lifecycle.releasing_models().is_empty());

    // S3: with nothing resident, a request for the unloaded id is admitted on
    // the fallback reservation and fails its lookup.
    let probe = infer_reply(&mut c, 2, "a").await;
    assert_eq!(
        probe.error_code,
        Some(InferenceErrorCode::ModelNotLoaded),
        "{probe:?}"
    );

    // 9. `b` is admitted after release and serves.
    let b = instance().expect("same fixture");
    d.lifecycle
        .load("b".into(), meta(), b)
        .await
        .expect("b fits after release");
    let served = infer_reply(&mut c, 3, "b").await;
    assert!(served.error.is_none(), "{served:?}");
    d.shutdown.cancel();
}
