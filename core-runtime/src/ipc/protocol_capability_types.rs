//! `capabilities_request` / `capabilities_response` payload (additive in V1;
//! #49 / #50). Kept out of `protocol_types.rs` (Section 4 Razor).
//!
//! `snapshot` is opaque JSON produced only by serializing the server's
//! output-only `models::capability_snapshot::CapabilitySnapshot`. The server
//! never deserializes inbound capability data into authority types; a
//! client-sent `capabilities_response` is refused as an unexpected message.
//! `snapshot.schema_version` is the capability schema version, not the IPC
//! protocol version.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CapabilitiesResponse {
    pub snapshot: serde_json::Value,
}

impl CapabilitiesResponse {
    pub fn new(snapshot: serde_json::Value) -> Self {
        Self { snapshot }
    }
}
