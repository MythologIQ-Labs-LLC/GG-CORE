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

use super::backend_dispatch::load_model_dispatch;
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
pub struct PreparedModel {
    pub model_id: String,
    pub metadata: ModelMetadata,
    pub model: Arc<dyn Model>,
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
    let validated = loader.validate_path(relative_path)?;
    let metadata = loader.load_metadata(&validated)?;
    let id = model_id.unwrap_or_else(|| metadata.name.clone());

    let model = load_model_dispatch(validated.as_path(), &id)
        .map_err(|e| ModelOpError::Backend(e.to_string()))?;

    Ok(PreparedModel {
        model_id: id,
        metadata,
        model,
    })
}

/// Register a prepared model atomically via the lifecycle coordinator.
pub async fn register_prepared(
    lifecycle: &Arc<ModelLifecycle>,
    prepared: PreparedModel,
) -> Result<LoadedModel, ModelOpError> {
    let handle = lifecycle
        .load(prepared.model_id.clone(), prepared.metadata, prepared.model)
        .await?;
    Ok(LoadedModel {
        model_id: prepared.model_id,
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
    let prepared = prepare_model(loader, relative_path, model_id)?;
    register_prepared(lifecycle, prepared).await
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
mod tests {
    use super::*;
    use crate::engine::InferenceEngine;
    use crate::models::registry::ModelRegistry;

    fn test_lifecycle() -> Arc<ModelLifecycle> {
        let registry = Arc::new(ModelRegistry::new());
        let engine = Arc::new(InferenceEngine::new(4096));
        Arc::new(ModelLifecycle::new(registry, engine))
    }

    #[tokio::test]
    async fn traversal_path_is_rejected_before_any_backend_work() {
        let loader = ModelLoader::new(std::path::PathBuf::from("."));
        let lifecycle = test_lifecycle();
        let err = load_model_from_path(&loader, &lifecycle, "../../etc/passwd", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ModelOpError::Path(_)), "got: {err}");
        assert_eq!(lifecycle.count().await, 0, "no partial state");
    }

    #[tokio::test]
    async fn nul_byte_path_is_rejected() {
        let loader = ModelLoader::new(std::path::PathBuf::from("."));
        let lifecycle = test_lifecycle();
        let err = load_model_from_path(&loader, &lifecycle, "models/a\0b.gguf", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ModelOpError::Path(_)));
    }

    #[tokio::test]
    async fn missing_model_fails_loud_with_no_partial_state() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("models")).unwrap();
        let loader = ModelLoader::new(dir.path().to_path_buf());
        let lifecycle = test_lifecycle();
        let err = load_model_from_path(&loader, &lifecycle, "models/absent.gguf", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ModelOpError::Path(LoadError::NotFound(_))));
        assert_eq!(lifecycle.count().await, 0);
    }

    #[tokio::test]
    async fn unload_of_absent_model_fails_loud() {
        let lifecycle = test_lifecycle();
        let err = unload_model(&lifecycle, "ghost").await.unwrap_err();
        assert!(matches!(
            err,
            ModelOpError::Lifecycle(LifecycleError::NotLoaded(_))
        ));
    }
}
