//! IPC model load/unload handlers (B-41, issue #106).
//!
//! Sidecar `impl IpcHandler` block (Section 4 Razor: `handler.rs` is at cap).
//! Both operations are dispatched only after `require_auth` in
//! `handle_message`. Failures are protocol-level responses (`success: false`)
//! so the connection survives; only authentication errors terminate it.

use tokio::task::spawn_blocking;

use super::handler::IpcHandler;
use super::protocol_model_types::{
    ModelLoadRequest, ModelLoadResponse, ModelUnloadRequest, ModelUnloadResponse,
};
use crate::models::model_ops;

impl IpcHandler {
    /// Load a model from a `base_path`-relative path.
    ///
    /// Path validation and backend dispatch run on a blocking thread so the
    /// IPC task stays responsive; registration is atomic via the lifecycle
    /// coordinator (`AlreadyLoaded` under its write lock; no partial state).
    pub(super) async fn handle_model_load(&self, request: ModelLoadRequest) -> ModelLoadResponse {
        let loader = std::sync::Arc::clone(&self.model_loader);
        let ModelLoadRequest { path, model_id } = request;

        let prepared = match spawn_blocking(move || {
            model_ops::prepare_model(&loader, &path, model_id)
        })
        .await
        {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(e)) => return ModelLoadResponse::error(e.to_string()),
            Err(join_err) => {
                return ModelLoadResponse::error(format!("load task failed: {join_err}"))
            }
        };

        match model_ops::register_prepared(&self.model_lifecycle, prepared).await {
            Ok(loaded) => ModelLoadResponse::success(loaded.model_id, loaded.handle_id),
            Err(e) => ModelLoadResponse::error(e.to_string()),
        }
    }

    /// Unload a loaded model by id.
    pub(super) async fn handle_model_unload(
        &self,
        request: ModelUnloadRequest,
    ) -> ModelUnloadResponse {
        match model_ops::unload_model(&self.model_lifecycle, &request.model_id).await {
            Ok(()) => ModelUnloadResponse::success(request.model_id),
            Err(e) => ModelUnloadResponse::error(request.model_id, e.to_string()),
        }
    }
}
