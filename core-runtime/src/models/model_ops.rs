//! Canonical standalone model load/unload operations (B-41, issue #106).
//!
//! One shared sequence for every out-of-process load path (daemon preload,
//! authenticated IPC load): validate the caller-supplied relative path against
//! the loader's `base_path` allowlist, read metadata, dispatch to the GGUF/ONNX
//! backend (optional sibling `manifest.json`), then register atomically through
//! the `ModelLifecycle` coordinator. No step leaves partial registry state on
//! failure: backend dispatch happens before any registration, and
//! `ModelLifecycle::load` checks/inserts under its index write lock.

use std::sync::Arc;

use super::backend_dispatch::load_model_dispatch_for_workers;
use super::lifecycle::{LifecycleError, ModelLifecycle};
use super::loader::{LoadError, ModelLoader, ModelMetadata};
use crate::engine::Model;

/// Outcome of a successful load.
#[derive(Debug, Clone)]
pub struct LoadedModel {
    pub model_id: String,
    pub handle_id: u64,
}

/// A validated, backend-loaded model awaiting lifecycle registration.
///
/// Field order matters: `model` drops before `reservation`, so an abandoned
/// load (failed registration, cancelled caller) frees backend memory before
/// its budget is returned.
pub struct PreparedModel {
    pub model_id: String,
    pub metadata: ModelMetadata,
    pub model: Arc<dyn Model>,
    /// Budget taken before backend allocation; resized at registration.
    reservation: Option<crate::memory::ResidentGuard>,
}

#[cfg(test)]
impl PreparedModel {
    /// Test-only: a prepared model holding `reservation`.
    pub(crate) fn for_test(
        model_id: &str,
        model: Arc<dyn Model>,
        reservation: Option<crate::memory::ResidentGuard>,
    ) -> Self {
        let metadata = ModelMetadata {
            name: model_id.into(),
            size_bytes: model.memory_usage() as u64,
        };
        Self {
            model_id: model_id.into(),
            metadata,
            model,
            reservation,
        }
    }
}

/// Unified error for the standalone load/unload operations.
#[derive(Debug, thiserror::Error)]
pub enum ModelOpError {
    #[error("path rejected: {0}")]
    Path(#[from] LoadError),
    #[error("backend load failed: {0}")]
    Backend(String),
    #[error("lifecycle: {0}")]
    Lifecycle(#[from] LifecycleError),
    /// Another operator load, unload or switch held the authority too long.
    #[error("model operation busy: another load, unload or switch is in progress")]
    Busy,
}

/// Validate the path, read metadata, and run backend dispatch (blocking).
///
/// This is the synchronous, potentially slow half of a load; async callers
/// should run it under `tokio::task::spawn_blocking` and then register the
/// result with [`register_prepared`]. No registry state is touched here, so
/// a failure leaves nothing to roll back.
pub fn prepare_model(
    loader: &ModelLoader,
    relative_path: &str,
    model_id: Option<String>,
) -> Result<PreparedModel, ModelOpError> {
    prepare_model_admitted(loader, None, relative_path, model_id)
}

/// [`prepare_model`] that first **reserves** the model's file size against
/// the total-memory budget, before any backend allocation. Concurrent loads
/// therefore cannot jointly exceed the budget (and hit the Windows Job Object
/// cap inside the backend); a load that does not fit fails explicitly. The
/// reservation travels with the [`PreparedModel`]: it is released if backend
/// load fails or the prepared model is dropped, and re-sized to the backend's
/// reported footprint when `ModelLifecycle` registers it. Single-context
/// GGUF threads; see [`prepare_model_admitted_for_workers`].
pub fn prepare_model_admitted(
    loader: &ModelLoader,
    limits: Option<&crate::memory::ResourceLimits>,
    relative_path: &str,
    model_id: Option<String>,
) -> Result<PreparedModel, ModelOpError> {
    prepare_model_admitted_for_workers(loader, limits, relative_path, model_id, 1)
}

/// [`prepare_model_admitted`] for a model `workers` concurrent contexts will
/// share (the daemon passes its worker-loop count; HOL plan Addendum A).
pub fn prepare_model_admitted_for_workers(
    loader: &ModelLoader,
    limits: Option<&crate::memory::ResourceLimits>,
    relative_path: &str,
    model_id: Option<String>,
    workers: usize,
) -> Result<PreparedModel, ModelOpError> {
    let validated = loader.validate_path(relative_path)?;
    let metadata = loader.load_metadata(&validated)?;
    let id = model_id.unwrap_or_else(|| metadata.name.clone());
    let reservation = match limits {
        Some(limits) => Some(reserve_for_load(limits, metadata.size_bytes)?),
        None => None,
    };

    let model = load_model_dispatch_for_workers(validated.as_path(), &id, workers)
        .map_err(|e| ModelOpError::Backend(e.to_string()))?;

    Ok(PreparedModel {
        model_id: id,
        metadata,
        model,
        reservation,
    })
}

fn reserve_for_load(
    limits: &crate::memory::ResourceLimits,
    size_bytes: u64,
) -> Result<crate::memory::ResidentGuard, ModelOpError> {
    let bytes = usize::try_from(size_bytes).unwrap_or(usize::MAX);
    limits
        .reserve_resident(bytes)
        .map_err(|e| ModelOpError::Lifecycle(LifecycleError::ResourceExhausted(e.to_string())))
}

/// Register a prepared model atomically via the lifecycle coordinator.
pub async fn register_prepared(
    lifecycle: &Arc<ModelLifecycle>,
    prepared: PreparedModel,
) -> Result<LoadedModel, ModelOpError> {
    let PreparedModel {
        model_id,
        metadata,
        model,
        reservation,
    } = prepared;
    let handle = lifecycle
        .load_reserved(model_id.clone(), metadata, model, reservation)
        .await?;
    Ok(LoadedModel {
        model_id,
        handle_id: handle.id(),
    })
}

/// Load a model from a `base_path`-relative path and register it.
///
/// `model_id` overrides the default id (the model file stem). Backend
/// dispatch runs on the calling thread; async contexts serving other traffic
/// should use [`prepare_model`] under `spawn_blocking` instead.
pub async fn load_model_from_path(
    loader: &ModelLoader,
    lifecycle: &Arc<ModelLifecycle>,
    relative_path: &str,
    model_id: Option<String>,
) -> Result<LoadedModel, ModelOpError> {
    load_model_from_path_for_workers(loader, lifecycle, relative_path, model_id, 1).await
}

/// [`load_model_from_path`] for `workers` concurrent contexts (daemon).
pub async fn load_model_from_path_for_workers(
    loader: &ModelLoader,
    lifecycle: &Arc<ModelLifecycle>,
    relative_path: &str,
    model_id: Option<String>,
    workers: usize,
) -> Result<LoadedModel, ModelOpError> {
    let limits = lifecycle.resource_limits();
    let prepared =
        prepare_model_admitted_for_workers(loader, limits, relative_path, model_id, workers)?;
    register_prepared(lifecycle, prepared).await
}

/// Operator load (daemon preload, IPC): take the mutation authority within
/// `wait`, prepare off the async runtime with the source hashed before the
/// load (identity re-checked after), register, and record the verified
/// source so a later cold switch can restore exactly this model.
pub async fn load_model_operated(
    lifecycle: &Arc<ModelLifecycle>,
    loader: &Arc<ModelLoader>,
    relative_path: &str,
    model_id: Option<String>,
    workers: usize,
    wait: std::time::Duration,
) -> Result<LoadedModel, ModelOpError> {
    use super::transition::{CanonicalPreparer, Preparer};
    let _authority = lifecycle
        .begin_operation_within(wait)
        .await
        .ok_or(ModelOpError::Busy)?;
    let prep = CanonicalPreparer {
        loader: Arc::clone(loader),
        workers,
    };
    let (path, limits) = (
        relative_path.to_string(),
        lifecycle.resource_limits().cloned(),
    );
    let (prepared, source) =
        tokio::task::spawn_blocking(move || prep.prepare_sourced(&path, model_id, limits.as_ref()))
            .await
            .map_err(|e| ModelOpError::Backend(format!("load task failed: {e}")))??;
    let loaded = register_prepared(lifecycle, prepared).await?;
    if let Some(handle) = lifecycle.get_handle(&loaded.model_id).await {
        lifecycle.record_source(handle, source);
    }
    Ok(loaded)
}

/// Operator unload: take the mutation authority within `wait`, then unload.
pub async fn unload_model_operated(
    lifecycle: &Arc<ModelLifecycle>,
    model_id: &str,
    wait: std::time::Duration,
) -> Result<(), ModelOpError> {
    let _authority = lifecycle
        .begin_operation_within(wait)
        .await
        .ok_or(ModelOpError::Busy)?;
    unload_model(lifecycle, model_id).await
}

/// Unload a previously loaded model by id.
pub async fn unload_model(
    lifecycle: &Arc<ModelLifecycle>,
    model_id: &str,
) -> Result<(), ModelOpError> {
    lifecycle.unload(model_id).await?;
    Ok(())
}

#[cfg(test)]
#[path = "model_ops_tests.rs"]
mod tests;
