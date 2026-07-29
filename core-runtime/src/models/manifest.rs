//! Model manifest parsing and validation for CORE Runtime.
//!
//! Manifests describe model metadata, capabilities, and integrity hashes.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::engine::error::InferenceError;

/// Model metadata from manifest.json file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Unique model identifier (e.g., "email-classifier-v1").
    pub model_id: String,
    /// Human-readable name.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Model capabilities.
    pub capabilities: Vec<ModelCapability>,
    /// SHA-256 hash of the model file.
    pub sha256: String,
    /// Size in bytes on disk.
    pub size_bytes: u64,
    /// Model architecture/format.
    pub architecture: ModelArchitecture,
    /// License identifier (SPDX).
    pub license: String,
    /// Ordered class labels for sequence-classification models
    /// (label `i` corresponds to logit `i`). Required when the manifest
    /// declares `TextClassification` and is dispatched to an ONNX classifier;
    /// `None`/absent for non-classifier models.
    #[serde(default)]
    pub labels: Option<Vec<String>>,
}

/// What a model can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    TextClassification,
    TextGeneration,
    Embedding,
    NamedEntityRecognition,
}

/// Model file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelArchitecture {
    /// llama.cpp quantized format (Q4, Q5, Q8 variants).
    Gguf,
    /// ONNX Runtime format.
    Onnx,
    /// HuggingFace SafeTensors format.
    SafeTensors,
}

impl ModelManifest {
    /// Load manifest from a JSON file.
    pub fn from_file(path: &Path) -> Result<Self, InferenceError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| InferenceError::ModelError(format!("failed to read manifest: {}", e)))?;
        Self::from_json(&content)
    }

    /// Parse manifest from JSON string.
    pub fn from_json(json: &str) -> Result<Self, InferenceError> {
        serde_json::from_str(json)
            .map_err(|e| InferenceError::ModelError(format!("invalid manifest JSON: {}", e)))
    }

    /// Validate manifest fields for correctness.
    pub fn validate(&self) -> Result<(), InferenceError> {
        if self.model_id.is_empty() {
            return Err(InferenceError::ModelError(
                "model_id cannot be empty".into(),
            ));
        }
        if self.sha256.len() != 64 {
            return Err(InferenceError::ModelError(
                "sha256 must be 64 hex characters".into(),
            ));
        }
        if self.capabilities.is_empty() {
            return Err(InferenceError::ModelError(
                "capabilities cannot be empty".into(),
            ));
        }
        Ok(())
    }

    /// Check if this model supports a specific capability.
    pub fn has_capability(&self, cap: ModelCapability) -> bool {
        self.capabilities.contains(&cap)
    }
}
