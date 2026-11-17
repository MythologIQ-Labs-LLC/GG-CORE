//! The model-preparation seam of the cold-switch coordinator.
//!
//! [`CanonicalPreparer`] is the production path: the canonical loader
//! (`base_path` confinement, manifest and sha256, budget reservation before
//! backend allocation). Tests substitute fakes so every refusal, rollback and
//! recovery branch is exercised without native model files.

use std::sync::Arc;

use crate::engine::Model;
use crate::memory::ResourceLimits;
use crate::models::backend_dispatch::{plan_backend, BackendChoice};
use crate::models::integrity::{check_identity_unchanged, hash_file};
use crate::models::lifecycle::ModelSource;
use crate::models::loader::ModelLoader;
use crate::models::model_ops::{prepare_model_admitted_for_workers, ModelOpError, PreparedModel};

/// What a non-disruptive preflight learned about a target, without loading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetFacts {
    /// On-disk size: the budget reservation the canonical load will take.
    pub(crate) size_bytes: u64,
}

/// Validates and prepares models for the coordinator (blocking calls).
pub(crate) trait Preparer: Send + Sync + 'static {
    /// Path, metadata and backend checks only; reads no model bytes.
    fn preflight(&self, relative_path: &str) -> Result<TargetFacts, ModelOpError>;

    /// Load through the canonical path and bind the result to its bytes.
    /// With `expected_sha256`, a file whose digest differs is refused before
    /// any backend reads it (rollback must never parse a substitute).
    fn prepare_sourced(
        &self,
        relative_path: &str,
        model_id: Option<String>,
        limits: Option<&ResourceLimits>,
        expected_sha256: Option<&str>,
    ) -> Result<(PreparedModel, ModelSource), ModelOpError>;

    /// Readiness of a freshly loaded model. The production check is the
    /// registration check alone; a smoke inference is an open #153 decision.
    fn verify_ready(&self, _model: &Arc<dyn Model>) -> Result<(), String> {
        Ok(())
    }
}

/// The production preparer over the daemon's real loader.
pub(crate) struct CanonicalPreparer {
    pub(crate) loader: Arc<ModelLoader>,
    pub(crate) workers: usize,
}

impl Preparer for CanonicalPreparer {
    fn preflight(&self, relative_path: &str) -> Result<TargetFacts, ModelOpError> {
        let validated = self.loader.validate_path(relative_path)?;
        let metadata = self.loader.load_metadata(&validated)?;
        match plan_backend(validated.as_path()) {
            Ok(BackendChoice::Unsupported(_)) => {
                Err(ModelOpError::Backend("unsupported model backend".into()))
            }
            Ok(_) => Ok(TargetFacts {
                size_bytes: metadata.size_bytes,
            }),
            Err(e) => Err(ModelOpError::Backend(e.to_string())),
        }
    }

    /// Bind the load to its bytes (audit R1). A manifest model's digest is
    /// the manifest's: the canonical load verifies it before the backend
    /// reads the file and re-checks identity after, so it is not hashed
    /// twice. A manifest-free model is hashed here first, then loaded, then
    /// its identity re-checked. Residual limit (as for manifest models):
    /// identity is length plus file id, so a same-length in-place rewrite of
    /// a trusted `models/` file between hash and load is not detected.
    fn prepare_sourced(
        &self,
        relative_path: &str,
        model_id: Option<String>,
        limits: Option<&ResourceLimits>,
        expected_sha256: Option<&str>,
    ) -> Result<(PreparedModel, ModelSource), ModelOpError> {
        let validated = self.loader.validate_path(relative_path)?;
        let path = validated.as_path();
        let declared = match plan_backend(path) {
            Ok(BackendChoice::GgufManifest(m) | BackendChoice::Onnx(m)) => Some(m.sha256),
            _ => None,
        };
        let (sha256, identity) = match declared {
            Some(digest) => (digest.to_ascii_lowercase(), None),
            None => {
                let (digest, identity) = hash_file(path).map_err(backend_err)?;
                (digest, Some(identity))
            }
        };
        if expected_sha256.is_some_and(|want| !want.eq_ignore_ascii_case(&sha256)) {
            return Err(ModelOpError::Backend(SOURCE_CHANGED.into()));
        }
        let prepared = prepare_model_admitted_for_workers(
            &self.loader,
            limits,
            relative_path,
            model_id,
            self.workers,
        )?;
        if let Some(identity) = identity {
            check_identity_unchanged(path, &identity).map_err(backend_err)?;
        }
        let source = ModelSource {
            relative_path: relative_path.to_string(),
            sha256,
        };
        Ok((prepared, source))
    }
}

/// Refusal text for a source file whose bytes no longer match its record.
pub(crate) const SOURCE_CHANGED: &str =
    "source file changed since it was loaded (sha256 mismatch); not loaded";

fn backend_err(e: crate::engine::InferenceError) -> ModelOpError {
    ModelOpError::Backend(e.to_string())
}
