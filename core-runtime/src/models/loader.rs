//! Model loading and validation.

use memmap2::Mmap;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

use super::containment;

/// Normalize a path lexically without resolving symlinks or hitting the filesystem.
/// This prevents TOCTOU vulnerabilities and bypasses from `canonicalize()`.
fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => normalized.push(p.as_os_str()),
            Component::RootDir => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // Return None if we try to `..` past the root or an empty path.
                // In absolute paths like those joined with base_path, this prevents escaping root.
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(c) => {
                normalized.push(c);
            }
        }
    }
    Some(normalized)
}

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
        // Reject NUL bytes at the validation seam: a path that is one string in
        // Rust but truncates at the first NUL across the C FFI boundary is a
        // validation-bypass class (the validated and used paths could differ).
        // The error payload is a fixed sentinel -- never echo a NUL-bearing path.
        if relative_path.contains('\0') {
            return Err(LoadError::PathNotAllowed(PathBuf::from(
                "<nul-byte rejected>",
            )));
        }
        let full_path = self.base_path.join(relative_path);

        // Lexically normalize the requested path to prevent path traversal
        // without relying on the OS/filesystem which could be vulnerable to TOCTOU.
        let normalized = normalize_path(&full_path)
            .ok_or_else(|| LoadError::PathNotAllowed(full_path.clone()))?;

        let is_allowed = ALLOWED_DIRS.iter().any(|dir| {
            let allowed = self.base_path.join(dir);
            // Lexically normalize the allowed path as well
            if let Some(allowed_normalized) = normalize_path(&allowed) {
                normalized.starts_with(&allowed_normalized)
            } else {
                false
            }
        });

        if !is_allowed {
            return Err(LoadError::PathNotAllowed(normalized));
        }

        // Physical containment: an existing entry must *resolve* under an
        // allowed root (symlinks and other indirection cannot escape). The
        // error carries the requested path only, never the resolved target.
        let roots: Vec<PathBuf> = ALLOWED_DIRS
            .iter()
            .map(|d| self.base_path.join(d))
            .collect();
        match containment::check(&normalized, &roots) {
            containment::Physical::Contained | containment::Physical::Absent => {
                Ok(ModelPath { path: normalized })
            }
            containment::Physical::Escapes => Err(LoadError::PathNotAllowed(normalized)),
        }
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
    /// Memory-map a model file for zero-copy access. The final path
    /// component is opened without following a symlink (Unix), so a link
    /// swapped in after validation is refused rather than mapped.
    pub fn open(path: &ModelPath) -> Result<Self, LoadError> {
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_NOFOLLOW);
        }
        let file: File = opts.open(path.as_path())?;
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

#[cfg(test)]
#[path = "loader_tests.rs"]
mod tests;
