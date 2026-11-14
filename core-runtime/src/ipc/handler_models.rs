//! IPC model load/unload handlers (B-41, issue #106).
//!
//! Sidecar `impl IpcHandler` block (Section 4 Razor: `handler.rs` is at cap).
//! Both operations are dispatched only after `require_auth` in
//! `handle_message`. Failures are protocol-level responses (`success: false`)
//! so the connection survives; only authentication errors terminate it.

use super::handler::IpcHandler;
use super::protocol_model_types::{
    ModelLoadRequest, ModelLoadResponse, ModelUnloadRequest, ModelUnloadResponse,
};
use crate::models::model_ops;

/// Longest an IPC load or unload waits for another operator mutation.
const OPERATION_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

impl IpcHandler {
    /// Load a model from a `base_path`-relative path.
    ///
    /// Operator path: takes the model-mutation authority (bounded wait, then
    /// a clear busy error), runs path validation and backend dispatch on a
    /// blocking thread, registers atomically, and records the verified
    /// source (sha256 hashed before the load) for a later cold switch.
    pub(super) async fn handle_model_load(&self, request: ModelLoadRequest) -> ModelLoadResponse {
        let ModelLoadRequest { path, model_id } = request;
        let limits = self.model_lifecycle.resource_limits();
        // The daemon's worker loops run on these same limits (Addendum A).
        let workers = crate::memory::ResourceLimits::worker_loops(limits);
        let loaded = model_ops::load_model_operated(
            &self.model_lifecycle,
            &self.model_loader,
            &path,
            model_id,
            workers,
            OPERATION_WAIT,
        )
        .await;
        match loaded {
            Ok(loaded) => ModelLoadResponse::success(loaded.model_id, loaded.handle_id),
            Err(e) => ModelLoadResponse::error(e.to_string()),
        }
    }

    /// Unload a loaded model by id (operator path: bounded authority wait).
    pub(super) async fn handle_model_unload(
        &self,
        request: ModelUnloadRequest,
    ) -> ModelUnloadResponse {
        let unloaded = model_ops::unload_model_operated(
            &self.model_lifecycle,
            &request.model_id,
            OPERATION_WAIT,
        )
        .await;
        match unloaded {
            Ok(()) => ModelUnloadResponse::success(request.model_id),
            Err(e) => ModelUnloadResponse::error(request.model_id, e.to_string()),
        }
    }

    /// Append unloaded instances whose memory is still held, as
    /// `unloading`. Read after the registry: an instance is pushed to the
    /// release tracker before it leaves the registry, and entries already
    /// seen in `models` (same never-reused handle) are skipped, so each held
    /// instance is listed exactly once.
    pub(super) fn append_releasing(&self, models: &mut Vec<crate::models::LoadedModelInfo>) {
        for info in self.model_lifecycle.releasing_models() {
            if !models.iter().any(|m| m.handle_id == info.handle_id) {
                models.push(info);
            }
        }
    }
}
