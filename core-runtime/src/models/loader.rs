//! Model loading and validation.

use memmap2::Mmap;
use std::fs::File;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoadError {
    #[error("Model path not allowed: {0}")]
    PathNotAllowed(PathBuf),

    #[error("Model file not found: {0}")]
    NotFound(PathBuf),

    #[error("Invalid model format: {0}")]
    InvalidFormat(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Validated model path within allowed directories.
#[derive(Debug, Clone)]
pub struct ModelPath {
    path: PathBuf,
}

impl ModelPath {
    pub fn as_path(&self) -> &Path {
        &self.path
    }
}

/// Allowed directories for model loading.
const ALLOWED_DIRS: &[&str] = &["models", "tokenizers"];

/// Loads and validates models from allowed directories.
pub struct ModelLoader {
    base_path: PathBuf,
}

impl ModelLoader {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Validate and create a ModelPath if within allowed directories.
    pub fn validate_path(&self, relative_path: &str) -> Result<ModelPath, LoadError> {
        let full_path = self.base_path.join(relative_path);
        let canonical = full_path
            .canonicalize()
            .map_err(|_| LoadError::NotFound(full_path.clone()))?;

        let is_allowed = ALLOWED_DIRS.iter().any(|dir| {
            let allowed = self.base_path.join(dir);
            // Canonicalize the allowed path to match format of canonical path
            allowed
                .canonicalize()
                .map(|allowed_canonical| canonical.starts_with(&allowed_canonical))
                .unwrap_or(false)
        });

        if !is_allowed {
            return Err(LoadError::PathNotAllowed(canonical));
        }

        Ok(ModelPath { path: canonical })
    }

    /// Load model metadata from validated path.
    pub fn load_metadata(&self, model_path: &ModelPath) -> Result<ModelMetadata, LoadError> {
        let path = model_path.as_path();

        if !path.exists() {
            return Err(LoadError::NotFound(path.to_path_buf()));
        }

        let size = std::fs::metadata(path)?.len();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(ModelMetadata {
            name,
            size_bytes: size,
        })
    }

    /// Load model using memory-mapping (zero-copy).
    /// Returns a MappedModel that provides direct access to file contents.
    pub fn load_mapped(&self, model_path: &ModelPath) -> Result<MappedModel, LoadError> {
        MappedModel::open(model_path)
    }
}

/// Basic model metadata.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    pub name: String,
    pub size_bytes: u64,
}

/// Memory-mapped model for zero-copy loading.
/// Uses memmap2 for cross-platform support.
pub struct MappedModel {
    mmap: Mmap,
}

// SAFETY: Mmap is Send+Sync when underlying file is read-only and not modified.
// We only use read-only mappings and models are immutable during inference.
unsafe impl Send for MappedModel {}
unsafe impl Sync for MappedModel {}

impl MappedModel {
    /// Memory-map a model file for zero-copy access.
    pub fn open(path: &ModelPath) -> Result<Self, LoadError> {
        let file = File::open(path.as_path())?;
        // SAFETY: File is opened read-only, model files are not modified during runtime
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(Self { mmap })
    }

    /// Get model data as a byte slice (zero-copy).
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }

    /// Length of mapped data in bytes.
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Check if mapped region is empty.
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }
}
