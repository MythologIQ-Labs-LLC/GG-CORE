//! Model lifecycle IPC payloads (B-41, issue #106).
//!
//! Request/response bodies for authenticated model load/unload over IPC.
//! Load/unload *failures* (rejected path, duplicate id, missing backend) are
//! ordinary responses with `success: false` — only authentication failures
//! terminate the connection (standard server behavior).

use serde::{Deserialize, Serialize};

/// Request to load a model from a `base_path`-relative path.
///
/// The path is constrained server-side by `ModelLoader::validate_path`
/// (NUL rejection, lexical normalization, `models/`/`tokenizers/` allowlist);
/// callers never gain filesystem traversal authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelLoadRequest {
    /// Path relative to the daemon's configured base path.
    pub path: String,
    /// Optional explicit model id; defaults to the model file stem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// Result of a load request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelLoadResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ModelLoadResponse {
    pub fn success(model_id: String, handle_id: u64) -> Self {
        Self {
            success: true,
            model_id: Some(model_id),
            handle_id: Some(handle_id),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            model_id: None,
            handle_id: None,
            error: Some(message.into()),
        }
    }
}

/// Request to unload a loaded model by id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelUnloadRequest {
    pub model_id: String,
}

/// Result of an unload request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelUnloadResponse {
    pub success: bool,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ModelUnloadResponse {
    pub fn success(model_id: String) -> Self {
        Self {
            success: true,
            model_id,
            error: None,
        }
    }

    pub fn error(model_id: String, message: impl Into<String>) -> Self {
        Self {
            success: false,
            model_id,
            error: Some(message.into()),
        }
    }
}
