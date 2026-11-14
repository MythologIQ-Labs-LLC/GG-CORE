//! Recovery after the target model failed to load or verify (#153 prototype).
//!
//! A registered-but-failed B is unloaded and must actually release before A
//! is reloaded (no double charge). A is restored only from its recorded
//! source, and only when the bytes read for the reload hash to the digest
//! recorded when A was first loaded (audit R1): never a substitute model.

use std::sync::Arc;

use super::{prepare, ColdSwitchRequest, LoadFailure, Preparer, SwitchOutcome};
use crate::models::lifecycle::{ModelLifecycle, ModelSource};
use crate::models::model_ops::register_prepared;

pub(super) async fn recover(
    lifecycle: &Arc<ModelLifecycle>,
    prep: &Arc<dyn Preparer>,
    req: &ColdSwitchRequest,
    source: &ModelSource,
    failure: LoadFailure,
) -> SwitchOutcome {
    let cause = match failure {
        LoadFailure::Unregistered(cause) => cause,
        LoadFailure::Registered { handle, cause } => {
            if let Err(pending) = retire_target(lifecycle, req, handle).await {
                return SwitchOutcome::RollbackFailed {
                    cause,
                    rollback_error: format!(
                        "target release pending: {pending} instance(s) still held; \
                         source not restored"
                    ),
                };
            }
            cause
        }
    };
    restore_source(lifecycle, prep, req, source, cause).await
}

/// Unload the failed target and wait (bounded) for its real release.
async fn retire_target(
    lifecycle: &ModelLifecycle,
    req: &ColdSwitchRequest,
    handle: crate::models::ModelHandle,
) -> Result<(), usize> {
    let _ = lifecycle.unload_expected(&req.to_id, handle).await;
    match lifecycle.wait_released(&req.to_id, req.release_wait).await {
        (true, _) => Ok(()),
        (false, pending) => Err(pending),
    }
}

/// Reload A from its recorded path; refuse a file whose bytes changed.
async fn restore_source(
    lifecycle: &Arc<ModelLifecycle>,
    prep: &Arc<dyn Preparer>,
    req: &ColdSwitchRequest,
    source: &ModelSource,
    cause: String,
) -> SwitchOutcome {
    let failed = |rollback_error: String, cause: String| SwitchOutcome::RollbackFailed {
        cause,
        rollback_error,
    };
    let (prepared, found) =
        match prepare(lifecycle, prep, &source.relative_path, &req.from_id).await {
            Ok(pair) => pair,
            Err(e) => return failed(format!("source reload failed: {e}"), cause),
        };
    if found.sha256 != source.sha256 {
        drop(prepared); // never registered: no handle, budget returned
        let msg = "source file changed since it was loaded (sha256 mismatch); not restored";
        return failed(msg.into(), cause);
    }
    match register_prepared(lifecycle, prepared).await {
        Ok(loaded) => {
            if let Some(handle) = lifecycle.get_handle(&req.from_id).await {
                lifecycle.record_source(handle, found);
            }
            SwitchOutcome::RolledBack {
                cause,
                restored_handle: loaded.handle_id,
            }
        }
        Err(e) => failed(format!("source registration failed: {e}"), cause),
    }
}
