//! `capabilities_request` handler (sidecar `impl IpcHandler`; Section 4
//! Razor: `handler.rs` is over cap). Dispatched after `require_auth`.

use super::handler::IpcHandler;
use super::protocol_capability_types::CapabilitiesResponse;
use crate::models::capability_snapshot::capability_snapshot;

impl IpcHandler {
    /// Serialize the output-only capability snapshot into an opaque value.
    pub(super) async fn handle_capabilities(&self) -> CapabilitiesResponse {
        let snapshot = capability_snapshot(
            &self.model_lifecycle,
            &self.inference_engine,
            self.config.require_auth,
        )
        .await;
        // Serializing plain structs of strings, integers and enums cannot fail;
        // `null` would be reported rather than a fabricated snapshot.
        let value = serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null);
        CapabilitiesResponse::new(value)
    }
}
