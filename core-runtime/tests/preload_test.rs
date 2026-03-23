//! Tests for ModelPreloader - preload validation before hot-swap.

use std::sync::Arc;
use gg_core::models::{
    ModelArchitecture, ModelCapability, ModelManifest, ModelPreloader, ModelRegistry, PreloadError,
};

fn test_manifest() -> ModelManifest {
    ModelManifest {
        model_id: "test-model".to_string(),
        name: "Test Model".to_string(),
        version: "1.0.0".to_string(),
        capabilities: vec![ModelCapability::TextGeneration],
        sha256: "a".repeat(64),
        size_bytes: 1024,
        architecture: ModelArchitecture::Gguf,
        license: "MIT".to_string(),
    }
}

#[tokio::test]
async fn test_preload_registers_handle() {
    let registry = Arc::new(ModelRegistry::new());
    let preloader = ModelPreloader::new(registry.clone());

    let manifest = test_manifest();
    let result = preloader.preload(manifest.clone()).await;

    assert!(result.is_ok());
    let preloaded = result.unwrap();
    assert!(registry.contains(preloaded.handle).await);
    assert_eq!(preloaded.manifest.model_id, "test-model");
}

#[tokio::test]
async fn test_preload_different_models_get_different_handles() {
    let registry = Arc::new(ModelRegistry::new());
    let preloader = ModelPreloader::new(registry.clone());

    let mut manifest1 = test_manifest();
    manifest1.model_id = "model-1".to_string();

    let mut manifest2 = test_manifest();
    manifest2.model_id = "model-2".to_string();

    let preloaded1 = preloader.preload(manifest1).await.unwrap();
    let preloaded2 = preloader.preload(manifest2).await.unwrap();

    assert_ne!(preloaded1.handle, preloaded2.handle);
}

#[tokio::test]
async fn test_abort_unregisters_handle() {
    let registry = Arc::new(ModelRegistry::new());
    let preloader = ModelPreloader::new(registry.clone());

    let manifest = test_manifest();
    let preloaded = preloader.preload(manifest).await.unwrap();
    let handle = preloaded.handle;

    assert!(registry.contains(handle).await);

    preloader.abort(preloaded).await;

    assert!(!registry.contains(handle).await);
}

#[tokio::test]
async fn test_preload_invalid_manifest_model_id() {
    let registry = Arc::new(ModelRegistry::new());
    let preloader = ModelPreloader::new(registry.clone());

    let mut manifest = test_manifest();
    manifest.model_id = "".to_string();

    let result = preloader.preload(manifest).await;
    assert!(matches!(result, Err(PreloadError::ManifestInvalid(_))));
}

#[tokio::test]
async fn test_preload_invalid_manifest_sha256() {
    let registry = Arc::new(ModelRegistry::new());
    let preloader = ModelPreloader::new(registry.clone());

    let mut manifest = test_manifest();
    manifest.sha256 = "invalid".to_string();

    let result = preloader.preload(manifest).await;
    assert!(matches!(result, Err(PreloadError::ManifestInvalid(_))));
}
