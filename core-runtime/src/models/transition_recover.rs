//! Recovery after the target model failed to load or verify (#153 prototype).
//!
//! A registered-but-failed B is unloaded and must actually release before A
//! is reloaded (no double charge). A is restored only from its recorded
//! source, and only when the bytes read for the reload hash to the digest
//! recorded when A was first loaded (audit R1): never a substitute model.

use std::sync::Arc;

use super::{prepare, ColdSwitchRequest, LoadFailure, Preparer, SwitchOutcome};
use crate::models::lifecycle::LifecycleError;
use crate::models::lifecycle::{ModelLifecycle, ModelSource};
use crate::models::model_ops::{register_prepared, ModelOpError};

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
            if let Err(rollback_error) = retire_target(lifecycle, req, handle).await {
                return SwitchOutcome::RollbackFailed {
                    cause,
                    rollback_error,
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
) -> Result<(), String> {
    // A zero pending-release count does not prove B was unpublished. In
    // particular, a bypass caller may have replaced B's observed handle.
    // Refuse recovery unless the exact failed instance was actually retired.
    lifecycle
        .unload_expected(&req.to_id, handle)
        .await
        .map_err(|err| format!("target unload failed: {err}; source not restored"))?;
    match lifecycle.wait_released(&req.to_id, req.release_wait).await {
        (true, _) => Ok(()),
        (false, pending) => Err(format!(
            "target release pending: {pending} instance(s) still held; source not restored"
        )),
    }
}

/// Reload A from its recorded path. The preparer refuses a file whose
/// digest differs from the record before any backend reads it.
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
    let expected = Some(source.sha256.clone());
    let path = &source.relative_path;
    let (prepared, found) = match prepare(lifecycle, prep, path, &req.from_id, expected).await {
        Ok(pair) => pair,
        Err(e) => return failed(format!("source reload refused or failed: {e}"), cause),
    };
    match register_prepared(lifecycle, prepared).await {
        Ok(loaded) => {
            let handle = crate::models::ModelHandle::new(loaded.handle_id);
            lifecycle.record_source(handle, found).await;
            SwitchOutcome::RolledBack {
                cause,
                restored_handle: loaded.handle_id,
            }
        }
        Err(ModelOpError::Lifecycle(LifecycleError::AlreadyLoaded(id))) => failed(
            format!(
                "another instance of '{id}' was loaded during recovery outside the operator authority; it serves, but it is not the verified source"
            ),
            cause,
        ),
        Err(e) => failed(format!("source registration failed: {e}"), cause),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::registry::ModelHandle;
    use crate::models::transition::support::{lifecycle, operator_load, request, target, Fake};

    #[tokio::test]
    async fn stale_failed_target_cannot_be_treated_as_released_for_rollback() {
        let (lc, limits) = lifecycle(1_000);
        let fake = Fake::with(&[
            ("a.gguf", target(400, "sha-a")),
            ("b.gguf", target(300, "sha-b")),
        ]);
        let a = operator_load(&lc, &fake, "a.gguf", "a").await;
        let source = lc.source(a).expect("source recorded");
        lc.unload_expected("a", a).await.expect("A retired");

        // An embedded bypass replaced the failed B with a different handle.
        // There is no pending release for "b"; a pending-only check reports
        // success even though a different live B is still registered.
        let replacement = operator_load(&lc, &fake, "b.gguf", "b").await;
        let prep: Arc<dyn Preparer> = fake.clone();
        let req = request("a", a, "b.gguf", "b");
        let out = recover(
            &lc,
            &prep,
            &req,
            &source,
            LoadFailure::Registered {
                handle: ModelHandle::new(replacement.id() + 99),
                cause: "failed registration check".into(),
            },
        )
        .await;

        let SwitchOutcome::RollbackFailed { rollback_error, .. } = out else {
            panic!("stale B must refuse recovery, got {out:?}");
        };
        assert!(
            rollback_error.contains("target unload failed"),
            "{rollback_error}"
        );
        assert_eq!(lc.get_handle("b").await, Some(replacement));
        assert!(lc.get_handle("a").await.is_none());
        assert_eq!(
            limits.current_memory(),
            300,
            "only replacement B is charged"
        );
    }
}
