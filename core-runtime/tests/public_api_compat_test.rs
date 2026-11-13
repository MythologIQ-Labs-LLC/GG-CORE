//! Downstream-consumer compatibility (HOL plan Addendum B): the public model
//! load API keeps its pre-#179 signatures. This integration test links
//! `gg_core` exactly as an external crate does; a signature change fails to
//! compile here.

use std::path::Path;
use std::sync::Arc;

use gg_core::engine::{InferenceError, Model};
use gg_core::memory::ResourceLimits;
use gg_core::models::{
    load_model_dispatch, load_model_from_path, prepare_model_admitted, ModelLoader, ModelOpError,
    PreparedModel,
};
use gg_core::{Runtime, RuntimeConfig};

type Dispatch = fn(&Path, &str) -> Result<Arc<dyn Model>, InferenceError>;
type Admitted = fn(
    &ModelLoader,
    Option<&ResourceLimits>,
    &str,
    Option<String>,
) -> Result<PreparedModel, ModelOpError>;

#[test]
fn original_signatures_bind_to_their_exact_types() {
    let _dispatch: Dispatch = load_model_dispatch;
    let _admitted: Admitted = prepare_model_admitted;
}

#[test]
fn original_sync_calls_fail_loud_on_a_missing_model() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    assert!(load_model_dispatch(&dir.path().join("models/absent.gguf"), "m").is_err());
    let loader = ModelLoader::new(dir.path().to_path_buf());
    assert!(prepare_model_admitted(&loader, None, "models/absent.gguf", None).is_err());
}

#[tokio::test]
async fn original_async_load_fails_loud_with_no_registry_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    let runtime = Runtime::new(RuntimeConfig {
        base_path: dir.path().to_path_buf(),
        auth_token: "compat-unused".into(),
        ..Default::default()
    });
    let result = load_model_from_path(
        &runtime.model_loader,
        &runtime.model_lifecycle,
        "models/absent.gguf",
        Some("m".into()),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(runtime.model_registry.count().await, 0);
}

/// MODEL_RELEASE_V1_PLAN A12: the release work leaves the unload API, its
/// wire structs and the CLI entry point exactly as before.
#[tokio::test]
async fn unload_api_and_wire_structs_are_unchanged() {
    use gg_core::ipc::protocol::{ModelUnloadRequest, ModelUnloadResponse};
    // Signature check only: the future is built, never polled.
    let _unpolled = gg_core::cli::models_lifecycle_cmd::run_unload("no-socket", "m");
    let request = ModelUnloadRequest {
        model_id: "m".into(),
    };
    let ModelUnloadResponse {
        success,
        model_id,
        error,
    } = ModelUnloadResponse::success(request.model_id);
    assert!(success && model_id == "m" && error.is_none());

    let runtime = Runtime::new(RuntimeConfig {
        auth_token: "compat-unused".into(),
        ..Default::default()
    });
    let lifecycle: &gg_core::models::ModelLifecycle = &runtime.model_lifecycle;
    assert!(lifecycle.unload("absent").await.is_err());
    let unloaded = gg_core::models::unload_model(&runtime.model_lifecycle, "absent").await;
    assert!(unloaded.is_err());
}
