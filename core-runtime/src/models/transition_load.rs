//! Load-and-verify step shared by the switch and its recovery.

use std::sync::Arc;

use tokio::task::spawn_blocking;

use super::Preparer;
use crate::models::lifecycle::{ModelLifecycle, ModelSource};
use crate::models::model_ops::{register_prepared, PreparedModel};
use crate::models::registry::ModelHandle;

/// Why loading a model failed, and whether it was already registered.
pub(super) enum LoadFailure {
    /// Nothing was registered (path, manifest, hash, budget or backend).
    Unregistered(String),
    /// Registered, then failed the registration/readiness check.
    Registered { handle: ModelHandle, cause: String },
}

/// Prepare under `spawn_blocking`, register, record the verified source and
/// run the registration check.
pub(super) async fn load_checked(
    lifecycle: &Arc<ModelLifecycle>,
    prep: &Arc<dyn Preparer>,
    path: &str,
    id: &str,
) -> Result<ModelHandle, LoadFailure> {
    let (prepared, source) = prepare(lifecycle, prep, path, id)
        .await
        .map_err(LoadFailure::Unregistered)?;
    let model = Arc::clone(&prepared.model);
    register_prepared(lifecycle, prepared)
        .await
        .map_err(|e| LoadFailure::Unregistered(e.to_string()))?;
    let Some(handle) = lifecycle.get_handle(id).await else {
        return Err(LoadFailure::Unregistered(
            "registered model vanished".into(),
        ));
    };
    lifecycle.record_source(handle, source);
    let ready = prep.verify_ready(&model);
    drop(model);
    match ready {
        Ok(()) if lifecycle.validate_handle(&handle).await => Ok(handle),
        Ok(()) => Err(LoadFailure::Registered {
            handle,
            cause: "registration check failed".into(),
        }),
        Err(cause) => Err(LoadFailure::Registered { handle, cause }),
    }
}

/// The preparer's canonical load, off the async runtime.
pub(super) async fn prepare(
    lifecycle: &ModelLifecycle,
    prep: &Arc<dyn Preparer>,
    path: &str,
    id: &str,
) -> Result<(PreparedModel, ModelSource), String> {
    let (p, limits) = (Arc::clone(prep), lifecycle.resource_limits().cloned());
    let (path, id) = (path.to_string(), id.to_string());
    spawn_blocking(move || p.prepare_sourced(&path, Some(id), limits.as_ref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
