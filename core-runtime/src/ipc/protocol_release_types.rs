//! Wire bodies for waiting on the release of an unloaded model (#137).
//!
//! `model_release_wait_request` is authenticated. It answers once no
//! unloaded instance of `model_id` is still held by an in-flight request
//! (`released: true`), or when `wait_ms` (clamped to 120 000) elapses
//! (`released: false`, `pending_instances > 0`). "Released" means the last
//! reference was dropped, backend destructors ran and the memory budget was
//! returned; it is not a claim that the OS reclaimed every page.
//!
//! The structs are `#[non_exhaustive]`: build them with the constructors, so
//! later additive fields are not a breaking change for Rust consumers.

use serde::{Deserialize, Serialize};

/// Longest wait accepted over IPC, in milliseconds.
pub const MAX_RELEASE_WAIT_MS: u64 = 120_000;

/// Wait for an unloaded model's memory to be released.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ModelReleaseWaitRequest {
    pub model_id: String,
    #[serde(default)]
    pub wait_ms: u64,
}

impl ModelReleaseWaitRequest {
    pub fn new(model_id: impl Into<String>, wait_ms: u64) -> Self {
        Self {
            model_id: model_id.into(),
            wait_ms,
        }
    }

    /// The requested wait, clamped to [`MAX_RELEASE_WAIT_MS`].
    pub fn clamped_wait(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.wait_ms.min(MAX_RELEASE_WAIT_MS))
    }
}

/// Release state of a model id when the wait ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ModelReleaseWaitResponse {
    pub model_id: String,
    pub released: bool,
    pub pending_instances: usize,
}

impl ModelReleaseWaitResponse {
    pub fn new(model_id: impl Into<String>, released: bool, pending_instances: usize) -> Self {
        Self {
            model_id: model_id.into(),
            released,
            pending_instances,
        }
    }
}

#[cfg(test)]
#[path = "protocol_release_tests.rs"]
mod tests;
