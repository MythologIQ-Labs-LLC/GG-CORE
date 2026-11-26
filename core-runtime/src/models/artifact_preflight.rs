//! Operator-pinned artifact preflight; not model qualification or admission.

use super::backend_dispatch::read_sibling_manifest;
use super::integrity::{check_identity_unchanged, verify_manifest_hash, FileIdentity};
use super::{ModelArchitecture, ModelCapability, ModelLoader, ModelManifest, ModelPath};
use crate::engine::error::InferenceError;

/// A point-in-time match against an independently approved manifest.
/// This is not serializable, not a loaded model, and not an execution permit.
#[derive(Debug)]
pub struct PinnedGgufArtifact {
    path: ModelPath,
    identity: FileIdentity,
    manifest: ModelManifest,
}

impl PinnedGgufArtifact {
    /// The parsed manifest observed and matched during preflight.
    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    /// Detect path replacement since preflight using existing file identity.
    /// Does not rehash bytes or detect all in-place writes.
    pub fn recheck_path_identity(&self) -> Result<(), InferenceError> {
        check_identity_unchanged(self.path.as_path(), &self.identity)
    }
}

/// Compare the local artifact and sibling manifest against a host-owned pin.
///
/// The approval must come from a separate trusted source, never from IPC
/// input or the same untrusted model directory. This helper does not establish
/// that the approval is authentic, that the file is a usable GGUF, or that
/// inference is qualified. Canonical load-time checks remain mandatory.
pub fn verify_pinned_gguf_artifact(
    loader: &ModelLoader,
    relative_path: &str,
    approved: &ModelManifest,
) -> Result<PinnedGgufArtifact, InferenceError> {
    approved.validate()?;
    if approved.architecture != ModelArchitecture::Gguf
        || approved.capabilities.as_slice() != [ModelCapability::TextGeneration]
    {
        return Err(refused("unsupported pinned GGUF manifest"));
    }

    let path = loader
        .validate_path(relative_path)
        .map_err(|_| refused("model path rejected"))?;
    let metadata =
        std::fs::symlink_metadata(path.as_path()).map_err(|_| refused("model file unavailable"))?;
    if !metadata.is_file() {
        return Err(refused("model must be a regular file"));
    }

    let manifest = read_sibling_manifest(path.as_path())?
        .ok_or_else(|| refused("approved artifact requires a sibling manifest"))?;
    if &manifest != approved {
        return Err(refused("observed manifest differs from approved manifest"));
    }
    let identity = verify_manifest_hash(path.as_path(), &manifest)?;
    check_identity_unchanged(path.as_path(), &identity)?;
    Ok(PinnedGgufArtifact {
        path,
        identity,
        manifest,
    })
}

fn refused(reason: &str) -> InferenceError {
    InferenceError::ModelError(reason.into())
}

#[cfg(test)]
#[path = "artifact_preflight_tests.rs"]
mod tests;
