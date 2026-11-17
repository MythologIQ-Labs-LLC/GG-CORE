//! Authenticated cold-switch control path (#137 / #153).
//!
//! All mutations go through the existing canonical lifecycle and the
//! coordinator's operator authority. A switch is detached from its caller,
//! but the bounded, authenticated journal makes retries read-only.
//! A successful registration is explicitly "loaded_unverified", NOT ready.

use std::sync::Arc;
use std::time::Duration;

use super::handler::IpcHandler;
use super::protocol_switch_types::{ModelSwitchRequest, ModelSwitchStatusRequest};
use super::switch_journal::Admission;
use crate::ipc::protocol::IpcMessage;
use crate::models::registry::ModelHandle;
use crate::models::transition::{
    join_switch, spawn_cold_switch, CanonicalPreparer, ColdSwitchRequest, Preparer, SwitchOutcome,
};

impl IpcHandler {
    pub(super) fn start_model_switch(&self, request: ModelSwitchRequest) -> IpcMessage {
        if let Err(message) = request.validate() {
            return IpcMessage::Error {
                code: 400,
                message: message.into(),
            };
        }
        let operation_id = request.operation_id.clone();
        match self.switch_journal.reserve(request.clone()) {
            Admission::Repeat(existing) => return IpcMessage::ModelSwitchResponse(existing),
            Admission::Conflict => {
                return IpcMessage::Error {
                    code: 409,
                    message: "operation_id was already used for a different transition".into(),
                }
            }
            Admission::Capacity => {
                return IpcMessage::Error {
                    code: 429,
                    message: "switch operation journal is at capacity".into(),
                }
            }
            Admission::New(_) => {}
        }

        let workers =
            crate::memory::ResourceLimits::worker_loops(self.model_lifecycle.resource_limits());
        let prep: Arc<dyn Preparer> = Arc::new(CanonicalPreparer {
            loader: Arc::clone(&self.model_loader),
            workers,
        });
        let switch = ColdSwitchRequest {
            from_id: request.from_id,
            expected_handle: ModelHandle::new(request.expected_handle),
            to_path: request.to_path,
            to_id: request.to_id,
            release_wait: Duration::from_millis(request.release_wait_ms),
        };
        match spawn_cold_switch(Arc::clone(&self.model_lifecycle), prep, switch) {
            Ok(task) => {
                let journal = Arc::clone(&self.switch_journal);
                tokio::spawn(async move {
                    let outcome = join_switch(task).await;
                    journal.complete(&operation_id, outcome);
                });
            }
            Err(refusal) => {
                self.switch_journal
                    .complete(&operation_id, SwitchOutcome::Refused(refusal));
            }
        }
        // The task may have finished already; return the freshest state.
        match self.switch_journal.status(&operation_id) {
            Some(status) => IpcMessage::ModelSwitchResponse(status),
            None => IpcMessage::Error {
                code: 500,
                message: "switch status was not retained".into(),
            },
        }
    }

    pub(super) fn read_model_switch(&self, request: ModelSwitchStatusRequest) -> IpcMessage {
        if let Err(message) = request.validate() {
            return IpcMessage::Error {
                code: 400,
                message: message.into(),
            };
        }
        // Authentication precedes this function. We never enumerate IDs, and
        // do not persist them across a daemon restart. A missing operation
        // must not be re-executed as a form of status recovery.
        match self.switch_journal.status(&request.operation_id) {
            Some(status) => IpcMessage::ModelSwitchStatusResponse(status),
            None => IpcMessage::Error {
                code: 404,
                message: "operation not found in this daemon instance".into(),
            },
        }
    }
}
