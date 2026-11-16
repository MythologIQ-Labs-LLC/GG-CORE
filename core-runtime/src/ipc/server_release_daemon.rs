//! Test daemon for the release-wait tests: model `m` (the [`Blocking`]
//! stub) loaded, a worker running, and a configurable writer-stall bound.

use std::sync::Arc;
use std::time::Duration;

use super::release_stub::Blocking;
use crate::engine::{InferenceParams, Model};
use crate::ipc::connections::{ConnectionConfig, ConnectionPool, IpcServerConfig};
use crate::ipc::protocol::*;
use crate::memory::ResourceLimitsConfig;
use crate::models::ModelMetadata;
use crate::{Runtime, RuntimeConfig};

pub(super) const TOKEN: &str = "release-wait-integration-test-secret";

pub(super) struct Daemon {
    pub(super) handler: Arc<crate::ipc::IpcHandler>,
    pub(super) pool: Arc<ConnectionPool>,
    pub(super) lifecycle: Arc<crate::models::ModelLifecycle>,
    pub(super) shutdown: tokio_util::sync::CancellationToken,
    /// Stands in for a request that still holds the model.
    pub(super) holder: Arc<dyn Model>,
}

/// A daemon with the default writer-stall bound.
pub(super) async fn daemon() -> Daemon {
    daemon_with(crate::engine::DEFAULT_STREAM_STALL).await
}

/// A daemon whose connections poison a frame write after `stall`, with
/// model `m` (the [`Blocking`] stub) loaded and one worker running.
pub(super) async fn daemon_with(stall: Duration) -> Daemon {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.into(),
        ipc_server: IpcServerConfig {
            stream_stall: stall,
            ..IpcServerConfig::default()
        },
        resource_limits: ResourceLimitsConfig {
            max_memory_per_call: 1 << 20,
            max_total_memory: 64 << 20,
            max_concurrent: 2,
        },
        ..Default::default()
    });
    let meta = ModelMetadata {
        name: "blocking-weights".into(),
        size_bytes: 4096,
    };
    let lifecycle = Arc::clone(&runtime.model_lifecycle);
    let model: Arc<dyn Model> = Arc::new(Blocking);
    lifecycle
        .load("m".into(), meta, Arc::clone(&model))
        .await
        .expect("load");
    let shutdown = tokio_util::sync::CancellationToken::new();
    spawn_worker(&runtime, &shutdown);
    Daemon {
        handler: Arc::new(runtime.ipc_handler),
        pool: Arc::new(ConnectionPool::new(ConnectionConfig { max_connections: 4 })),
        lifecycle,
        shutdown,
        holder: model,
    }
}

/// A non-streaming request that holds `m` until it is cancelled.
pub(super) fn generate(id: u64) -> IpcMessage {
    IpcMessage::InferenceRequest(InferenceRequest {
        request_id: RequestId(id),
        model_id: "m".into(),
        prompt: "hold the model".into(),
        parameters: InferenceParams {
            timeout_ms: Some(60_000),
            ..Default::default()
        },
    })
}

/// One scheduler worker for the daemon, stopped by `shutdown`.
fn spawn_worker(runtime: &Runtime, shutdown: &tokio_util::sync::CancellationToken) {
    let _worker = crate::scheduler::spawn_worker_with_registry(
        runtime.request_queue.clone(),
        runtime.inference_engine.clone(),
        None,
        None,
        Some(runtime.resource_limits.clone()),
        Some(runtime.security.clone()),
        shutdown.clone(),
    );
}

#[path = "server_capability_tests.rs"]
mod capability_tests;
