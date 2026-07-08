//! Model preloading and validation for hot-swap operations.
//!
//! Preloads and validates models before they are swapped into active service.

use std::sync::Arc;
use thiserror::Error;

use super::manifest::ModelManifest;
use super::registry::{ModelHandle, ModelRegistry};

#[derive(Error, Debug)]
pub enum PreloadError {
    #[error("Manifest invalid: {0}")]
    ManifestInvalid(String),

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Load failed: {0}")]
    LoadFailed(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

/// Result of successful preload.
pub struct PreloadedModel {
    pub handle: ModelHandle,
    pub manifest: ModelManifest,
}

/// Preloads and validates models before swap.
pub struct ModelPreloader {
    registry: Arc<ModelRegistry>,
}

impl ModelPreloader {
    pub fn new(registry: Arc<ModelRegistry>) -> Self {
        Self { registry }
    }

    /// Preload model: validate manifest, register in registry.
    pub async fn preload(&self, manifest: ModelManifest) -> Result<PreloadedModel, PreloadError> {
        self.validate_manifest(&manifest)?;

        let metadata = super::loader::ModelMetadata {
            name: manifest.name.clone(),
            size_bytes: manifest.size_bytes,
        };

        let handle = self
            .registry
            .register(metadata, manifest.size_bytes as usize)
            .await;

        Ok(PreloadedModel { handle, manifest })
    }

    /// Validate manifest fields.
    fn validate_manifest(&self, manifest: &ModelManifest) -> Result<(), PreloadError> {
        if manifest.model_id.is_empty() {
            return Err(PreloadError::ManifestInvalid(
                "model_id cannot be empty".into(),
            ));
        }
        if manifest.sha256.len() != 64 {
            return Err(PreloadError::ManifestInvalid(
                "sha256 must be 64 hex characters".into(),
            ));
        }
        if manifest.capabilities.is_empty() {
            return Err(PreloadError::ManifestInvalid(
                "capabilities cannot be empty".into(),
            ));
        }
        Ok(())
    }

    /// Abort preload: unregister from registry, free resources.
    pub async fn abort(&self, preloaded: PreloadedModel) {
        self.registry.unregister(preloaded.handle).await;
    }
}
