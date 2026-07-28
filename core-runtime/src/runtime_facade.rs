//! Secure inference façade for the embedded surface.
//!
//! One enforced entry point — [`Runtime::infer`] / [`Runtime::infer_stream`] —
//! wraps the pure `InferenceEngine` with the shared `SecurityPipeline`. Ingress
//! scan → (block ⇒ typed `SecurityRejected`) → engine → egress sanitize. The
//! engine stays pure compute (C.O.R.E. charter); enforcement lives here and in
//! the scheduler worker via the SAME `Arc<SecurityPipeline>`.
//!
//! This module also owns the `Runtime` construction helpers (relocated from
//! `lib.rs` to keep it under the Section 4 line budget).

use std::sync::Arc;

use crate::engine::inference::InferenceError; // LD-5: the enum `run` returns
#[cfg(feature = "gguf")]
use crate::engine::{InferenceConfig, TokenStream};
use crate::engine::{InferenceParams, InferenceResult};
use crate::health::HealthChecker;
use crate::ipc::{IpcHandler, IpcHandlerConfig, SessionAuth};
use crate::memory::{ContextCache, GpuMemory, MemoryPool, ResourceLimits};
use crate::models::{ModelMetadata, ModelRegistry};
use crate::scheduler::{BatchProcessor, OutputCache, RequestQueue};
use crate::shutdown::ShutdownCoordinator;
use crate::telemetry::{self, MetricsStore};
use crate::{Runtime, RuntimeConfig};
use tokio::sync::Mutex;

/// Rejection message surfaced to callers. Leak-safe: no pattern names or matched
/// text. Matches the worker's rejection string (`scheduler/worker.rs`).
const REJECTION_MESSAGE: &str = "request rejected by security policy";

/// Build a SmartLoader callback that validates paths and registers
/// models in the given registry, producing globally unique handles.
pub(crate) fn build_loader_callback(
    registry: Arc<ModelRegistry>,
) -> crate::models::smart_loader_types::LoadCallback {
    Box::new(move |path| {
        if !path.exists() {
            return Err(format!("Model file not found: {}", path.display()));
        }
        let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let meta = ModelMetadata {
            name,
            size_bytes: size,
        };
        let handle = futures::executor::block_on(registry.register(meta, size as usize));
        Ok(handle)
    })
}

impl Runtime {
    /// Run inference through the secure façade.
    ///
    /// Ingress scan → (block ⇒ `SecurityRejected`) → engine → egress sanitize.
    pub async fn infer(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        let verdict = self.security.scan_prompt(prompt);
        telemetry::record_security_scan(
            model_id,
            verdict.latency_us,
            verdict.risk_score,
            !verdict.allowed,
        );
        if !verdict.allowed {
            return Err(InferenceError::SecurityRejected(REJECTION_MESSAGE.into()));
        }
        let mut result = self.inference_engine.run(model_id, prompt, params).await?;
        let s = self.security.sanitize_output(&result.output);
        telemetry::record_output_sanitize(model_id, s.latency_us, s.redactions as u64);
        result.output = s.output;
        Ok(result)
    }

    /// Start secured streaming inference.
    ///
    /// Scans ingress first; a blocked prompt returns `SecurityRejected` before
    /// any token is produced. On allow, spawns `run_stream_sync` on a blocking
    /// task and returns the receiver. Egress token sanitization is out of scope.
    ///
    /// # Precondition
    /// Must be called within a tokio runtime (uses `spawn_blocking`, and
    /// `run_stream_sync` calls `Handle::current()`).
    #[cfg(feature = "gguf")]
    pub fn infer_stream(
        &self,
        model_id: &str,
        prompt: &str,
        config: &InferenceConfig,
    ) -> Result<TokenStream, InferenceError> {
        let verdict = self.security.scan_prompt(prompt);
        telemetry::record_security_scan(
            model_id,
            verdict.latency_us,
            verdict.risk_score,
            !verdict.allowed,
        );
        if !verdict.allowed {
            return Err(InferenceError::SecurityRejected(REJECTION_MESSAGE.into()));
        }
        let (sender, stream) = TokenStream::new(32);
        let engine = self.inference_engine.clone();
        // Egress enforcement: the stream is detokenized + PII-sanitized in-runtime
        // and emitted as sanitized text (B-24b). Raw token ids never leave.
        let security = self.security.clone();
        let (mid, prompt, cfg) = (model_id.to_string(), prompt.to_string(), config.clone());
        tokio::task::spawn_blocking(move || {
            let _ = engine.run_stream_sync(&mid, &prompt, &cfg, sender, Some(security.as_ref()));
        });
        Ok(stream)
    }

    pub(crate) fn init_memory(config: &RuntimeConfig) -> (MemoryPool, GpuMemory, ContextCache) {
        (
            MemoryPool::new(config.memory_pool.clone()),
            GpuMemory::new(config.gpu_memory.clone()),
            ContextCache::new(config.context_cache.clone()),
        )
    }

    pub(crate) fn init_scheduler(
        config: &RuntimeConfig,
    ) -> (
        Arc<RequestQueue>,
        BatchProcessor,
        ResourceLimits,
        Arc<Mutex<OutputCache>>,
    ) {
        (
            Arc::new(RequestQueue::new(config.request_queue.clone())),
            BatchProcessor::new(config.batch.clone()),
            ResourceLimits::new(config.resource_limits.clone()),
            Arc::new(Mutex::new(OutputCache::new(config.output_cache.clone()))),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn init_ipc(
        config: &RuntimeConfig,
        queue: &Arc<RequestQueue>,
        shutdown: &Arc<ShutdownCoordinator>,
        health: &Arc<HealthChecker>,
        registry: &Arc<ModelRegistry>,
        metrics: &Arc<MetricsStore>,
        engine: &Arc<crate::engine::InferenceEngine>,
    ) -> IpcHandler {
        let session_auth = Arc::new(SessionAuth::new(&config.auth_token, config.session_timeout));
        IpcHandler::new(
            session_auth,
            queue.clone(),
            IpcHandlerConfig::default(),
            shutdown.clone(),
            health.clone(),
            registry.clone(),
            metrics.clone(),
            Arc::clone(engine),
        )
    }
}
