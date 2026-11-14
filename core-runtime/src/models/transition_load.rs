//! Load-and-verify steps shared by operator loads, the switch and recovery.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::spawn_blocking;

use super::Preparer;
use crate::models::lifecycle::{ModelLifecycle, ModelSource};
use crate::models::model_ops::{register_prepared, LoadedModel, ModelOpError, PreparedModel};
use crate::models::registry::ModelHandle;

/// Why loading a model failed, and whether it was already registered.
pub(super) enum LoadFailure {
    /// Nothing was registered (path, manifest, hash, budget or backend).
    Unregistered(String),
    /// Registered, then failed the registration/readiness check.
    Registered { handle: ModelHandle, cause: String },
}

/// Operator load (daemon preload, IPC): take the authority within `wait`,
/// prepare off the async runtime, register, and record the verified source
/// against exactly the registered handle.
pub(crate) async fn load_operated(
    lifecycle: &Arc<ModelLifecycle>,
    prep: &Arc<dyn Preparer>,
    path: &str,
    id: Option<String>,
    wait: Duration,
) -> Result<LoadedModel, ModelOpError> {
    let _authority = lifecycle
        .begin_operation_within(wait)
        .await
        .ok_or(ModelOpError::Busy)?;
    let (prepared, source) = prepare_blocking(lifecycle, prep, path, id, None).await?;
    let loaded = register_prepared(lifecycle, prepared).await?;
    let handle = ModelHandle::new(loaded.handle_id);
    lifecycle.record_source(handle, source).await;
    Ok(loaded)
}

/// Prepare under `spawn_blocking`, register, record the verified source and
/// run the registration check.
pub(super) async fn load_checked(
    lifecycle: &Arc<ModelLifecycle>,
    prep: &Arc<dyn Preparer>,
    path: &str,
    id: &str,
) -> Result<ModelHandle, LoadFailure> {
    let (prepared, source) = prepare(lifecycle, prep, path, id, None)
        .await
        .map_err(LoadFailure::Unregistered)?;
    let model = Arc::clone(&prepared.model);
    let loaded = register_prepared(lifecycle, prepared)
        .await
        .map_err(|e| LoadFailure::Unregistered(e.to_string()))?;
    let handle = ModelHandle::new(loaded.handle_id);
    let recorded = lifecycle.record_source(handle, source).await;
    let ready = prep.verify_ready(&model);
    drop(model);
    match ready {
        Ok(()) if recorded && lifecycle.validate_handle(&handle).await => Ok(handle),
        Ok(()) => Err(LoadFailure::Registered {
            handle,
            cause: "registration check failed".into(),
        }),
        Err(cause) => Err(LoadFailure::Registered { handle, cause }),
    }
}

/// The preparer's canonical load, off the async runtime (string errors).
pub(super) async fn prepare(
    lifecycle: &ModelLifecycle,
    prep: &Arc<dyn Preparer>,
    path: &str,
    id: &str,
    expected_sha256: Option<String>,
) -> Result<(PreparedModel, ModelSource), String> {
    prepare_blocking(lifecycle, prep, path, Some(id.into()), expected_sha256)
        .await
        .map_err(|e| e.to_string())
}

async fn prepare_blocking(
    lifecycle: &ModelLifecycle,
    prep: &Arc<dyn Preparer>,
    path: &str,
    id: Option<String>,
    expected_sha256: Option<String>,
) -> Result<(PreparedModel, ModelSource), ModelOpError> {
    let (p, limits, path) = (
        Arc::clone(prep),
        lifecycle.resource_limits().cloned(),
        path.to_string(),
    );
    spawn_blocking(move || {
        p.prepare_sourced(&path, id, limits.as_ref(), expected_sha256.as_deref())
    })
    .await
    .map_err(|e| ModelOpError::Backend(format!("load task failed: {e}")))?
}
