//! Tests for the canonical model load/unload operations.

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
