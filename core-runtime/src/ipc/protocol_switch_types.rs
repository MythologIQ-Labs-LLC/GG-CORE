//! Authenticated, idempotent cold-switch IPC contract (#137 / #153).
//!
//! An operator supplies a fresh unpredictable 64-hex operation ID and keeps
//! it across reconnects. Repeating exactly the same request cannot evict a
//! second model. Requesting status requires both normal IPC authentication
//! and possession of that operation ID. Entries are bounded and ephemeral.
//!
//! The first implementation must never claim B is *ready* solely because
//! the lifecycle registered a new model handle. "loaded_unverified" is
//! deliberate until a real, task-specific inference probe is qualified.

use serde::{Deserialize, Serialize};

pub const MAX_SWITCH_WAIT_MS: u64 = 120_000;
pub const SWITCH_OPERATION_ID_HEX_LEN: usize = 64;

fn valid_operation_id(id: &str) -> bool {
    id.len() == SWITCH_OPERATION_ID_HEX_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A one-time, operator-issued cold transition. No implicit cancellation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelSwitchRequest {
    pub operation_id: String,
    pub from_id: String,
    pub expected_handle: u64,
    pub to_path: String,
    pub to_id: String,
    pub release_wait_ms: u64,
}

impl ModelSwitchRequest {
    pub fn new(
        operation_id: String,
        from_id: String,
        expected_handle: u64,
        to_path: String,
        to_id: String,
        release_wait_ms: u64,
    ) -> Self {
        Self {
            operation_id,
            from_id,
            expected_handle,
            to_path,
            to_id,
            release_wait_ms,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_operation_id(&self.operation_id) {
            return Err("operation_id must be a fresh 64-character hex token");
        }
        if self.from_id.is_empty()
            || self.from_id.len() > 256
            || self.to_id.is_empty()
            || self.to_id.len() > 256
            || self.to_path.is_empty()
            || self.to_path.len() > 4096
        {
            return Err("invalid source ID, target ID, or relative target path");
        }
        if self.expected_handle == 0 {
            return Err("expected_handle must name a real observed model instance");
        }
        if self.release_wait_ms == 0 || self.release_wait_ms > MAX_SWITCH_WAIT_MS {
            return Err("release_wait_ms must be 1..=120000");
        }
        Ok(())
    }
}

/// Query an admitted operation after the original IPC connection disappears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelSwitchStatusRequest {
    pub operation_id: String,
}

impl ModelSwitchStatusRequest {
    pub fn new(operation_id: String) -> Self {
        Self { operation_id }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_operation_id(&self.operation_id) {
            return Err("operation_id must be 64 lowercase hex characters");
        }
        Ok(())
    }
}

/// Exact state, not an implicit claim that another model is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelSwitchPhase {
    Running,
    Refused,
    ReleasePending,
    LoadedUnverified,
    RolledBackUnverified,
    RollbackFailed,
    Unknown,
}

/// Immediate acceptance or a read-only, authenticated operation lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelSwitchStatusResponse {
    pub operation_id: String,
    pub phase: ModelSwitchPhase,
    /// Always false until a real task-appropriate smoke inference proves
    /// readiness for the exact handle under the current policy.
    pub ready: bool,
    pub from_id: String,
    pub to_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_handle: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ModelSwitchStatusResponse {
    pub fn new(operation_id: String, from_id: String, to_id: String) -> Self {
        Self {
            operation_id,
            phase: ModelSwitchPhase::Running,
            ready: false,
            from_id,
            to_id,
            to_handle: None,
            detail: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unbounded_or_ambiguous_transition_parameters() {
        let mut r = ModelSwitchRequest::new(
            "a".repeat(64),
            "a".into(),
            1,
            "models/b.gguf".into(),
            "b".into(),
            5000,
        );
        assert!(r.validate().is_ok());
        r.release_wait_ms = 120_001;
        assert!(r.validate().is_err());
        r.release_wait_ms = 5000;
        r.expected_handle = 0;
        assert!(r.validate().is_err());
        r.expected_handle = 1;
        r.operation_id = "not-secret".into();
        assert!(r.validate().is_err());
    }

    #[test]
    fn status_lookup_requires_bounded_lowercase_operation_id() {
        let valid = ModelSwitchStatusRequest::new("a".repeat(64));
        assert!(valid.validate().is_ok());
        for invalid in [
            "A".repeat(64),
            "g".repeat(64),
            "a".repeat(65),
            String::new(),
        ] {
            assert!(ModelSwitchStatusRequest::new(invalid).validate().is_err());
        }
        let mut req = ModelSwitchRequest::new(
            "A".repeat(64),
            "source".into(),
            1,
            "models/b.gguf".into(),
            "b".into(),
            5000,
        );
        assert!(req.validate().is_err());
        req.operation_id = "a".repeat(64);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn wire_roundtrip_does_not_claim_readiness() {
        let r = ModelSwitchStatusResponse::new("a".repeat(64), "a".into(), "b".into());
        let encoded = serde_json::to_string(&r).unwrap();
        let decoded: ModelSwitchStatusResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(r, decoded);
        assert!(!decoded.ready);
        assert_eq!(decoded.phase, ModelSwitchPhase::Running);
    }
}
