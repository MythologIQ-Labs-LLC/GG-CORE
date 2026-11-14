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
    fn prepare_sourced(
        &self,
        relative_path: &str,
        model_id: Option<String>,
        limits: Option<&ResourceLimits>,
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

    /// Hash before the load, load, then re-check the file identity, so the
    /// recorded digest is the digest of the bytes the backend actually read
    /// (audit R1). A manifest-declared digest is also verified by the load.
    fn prepare_sourced(
        &self,
        relative_path: &str,
        model_id: Option<String>,
        limits: Option<&ResourceLimits>,
    ) -> Result<(PreparedModel, ModelSource), ModelOpError> {
        let validated = self.loader.validate_path(relative_path)?;
        let (sha256, identity) =
            hash_file(validated.as_path()).map_err(|e| ModelOpError::Backend(e.to_string()))?;
        let prepared = prepare_model_admitted_for_workers(
            &self.loader,
            limits,
            relative_path,
            model_id,
            self.workers,
        )?;
        check_identity_unchanged(validated.as_path(), &identity)
            .map_err(|e| ModelOpError::Backend(e.to_string()))?;
        let source = ModelSource {
            relative_path: relative_path.to_string(),
            sha256,
        };
        Ok((prepared, source))
    }
}
