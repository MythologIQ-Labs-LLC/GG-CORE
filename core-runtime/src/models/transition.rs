//! Conservative cold-switch transition coordinator (#137 / #153).
//!
//! Crate-private and not exposed over IPC: #153 still owes owner decisions on
//! drain, cancellation, warm overlap and rollback. The switch never preempts
//! in-flight work, never hands KV state across models, never assumes memory
//! was reclaimed and never falls back silently. Plan and audit:
//! `docs/integration/V1_COLD_SWITCH_COORDINATOR_PLAN.md`.
//!
//! Order: authority → preconditions → non-disruptive preflight → close A to
//! new admissions → bounded wait for A's real release → load B → registration
//! check → (on failure) recover to A from its recorded, re-hashed source.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::{spawn_blocking, JoinHandle};

use super::lifecycle::{
    LifecycleError, ModelLifecycle, ModelSource, OperationGuard, MAX_RELEASE_WAIT,
};
use super::registry::ModelHandle;

pub(crate) use preparer::{CanonicalPreparer, Preparer, TargetFacts};

/// One operator request to switch the serving model from A to B.
#[derive(Debug, Clone)]
pub(crate) struct ColdSwitchRequest {
    pub(crate) from_id: String,
    /// The exact instance the operator observed; a replacement is refused.
    pub(crate) expected_handle: ModelHandle,
    pub(crate) to_path: String,
    pub(crate) to_id: String,
    /// Bound on waiting for A's in-flight requests to release its memory.
    pub(crate) release_wait: Duration,
}

/// Why a switch was refused before anything changed (A still serving).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchRefusal {
    Busy,
    NotLoaded,
    StaleHandle,
    /// A was not loaded by an operator path, so it has no verified source.
    UnknownSource,
    SameModel,
    TargetLoaded,
    WaitTooLong,
    Preflight(String),
    /// Budget estimate (re-checked at load): B cannot fit even after A's release.
    OverBudget {
        needed: u64,
        available: u64,
    },
}

/// The truthful end state of a switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchOutcome {
    /// B serves; A is unloaded and its memory was released.
    Switched {
        from_handle: u64,
        to_handle: u64,
        release_ms: u128,
        load_ms: u128,
    },
    /// Nothing changed; A still serves.
    Refused(SwitchRefusal),
    /// A is unloaded and still draining; B was not loaded (an outage: #153).
    ReleasePending { from_handle: u64, pending: usize },
    /// B failed; A serves again as a new instance from its verified source.
    RolledBack { cause: String, restored_handle: u64 },
    /// B failed and A could not be restored: neither model serves.
    RollbackFailed {
        cause: String,
        rollback_error: String,
    },
    /// The switch task ended abnormally; read the model listing for the state.
    Unknown { reason: String },
}

/// Take the operator authority and run the switch in its own task, so a
/// dropped caller cannot interrupt it midway or leave the authority held.
pub(crate) fn spawn_cold_switch(
    lifecycle: Arc<ModelLifecycle>,
    prep: Arc<dyn Preparer>,
    request: ColdSwitchRequest,
) -> Result<JoinHandle<SwitchOutcome>, SwitchRefusal> {
    let guard = lifecycle.try_begin_operation().ok_or(SwitchRefusal::Busy)?;
    Ok(tokio::spawn(run(lifecycle, prep, request, guard)))
}

/// Await a spawned switch; an abnormal end is reported as unknown state.
pub(crate) async fn join_switch(handle: JoinHandle<SwitchOutcome>) -> SwitchOutcome {
    handle.await.unwrap_or_else(|e| SwitchOutcome::Unknown {
        reason: format!(
            "switch task ended abnormally ({e}); the state is unknown: read the model \
             listing (loaded and unloading instances)"
        ),
    })
}

async fn run(
    lifecycle: Arc<ModelLifecycle>,
    prep: Arc<dyn Preparer>,
    req: ColdSwitchRequest,
    _authority: OperationGuard,
) -> SwitchOutcome {
    let source = match preconditions(&lifecycle, &req).await {
        Ok(source) => source,
        Err(refusal) => return SwitchOutcome::Refused(refusal),
    };
    if let Err(refusal) = preflight(&lifecycle, &prep, &req, &source).await {
        return SwitchOutcome::Refused(refusal);
    }
    // Disruption starts here: A closes to new admissions; in-flight and
    // streaming requests continue to completion.
    match lifecycle
        .unload_expected(&req.from_id, req.expected_handle)
        .await
    {
        Ok(_) => {}
        Err(LifecycleError::StaleHandle(_)) => {
            return SwitchOutcome::Refused(SwitchRefusal::StaleHandle)
        }
        Err(_) => return SwitchOutcome::Refused(SwitchRefusal::NotLoaded),
    }
    let released_at = Instant::now();
    let (released, pending) = lifecycle
        .wait_released(&req.from_id, req.release_wait)
        .await;
    if !released {
        return SwitchOutcome::ReleasePending {
            from_handle: req.expected_handle.id(),
            pending,
        };
    }
    let release_ms = released_at.elapsed().as_millis();
    let loading = Instant::now();
    match load_checked(&lifecycle, &prep, &req.to_path, &req.to_id).await {
        Ok(to) => SwitchOutcome::Switched {
            from_handle: req.expected_handle.id(),
            to_handle: to.id(),
            release_ms,
            load_ms: loading.elapsed().as_millis(),
        },
        Err(failure) => recover::recover(&lifecycle, &prep, &req, &source, failure).await,
    }
}

async fn preconditions(
    lifecycle: &ModelLifecycle,
    req: &ColdSwitchRequest,
) -> Result<ModelSource, SwitchRefusal> {
    if req.release_wait > MAX_RELEASE_WAIT {
        return Err(SwitchRefusal::WaitTooLong);
    }
    if req.to_id == req.from_id {
        return Err(SwitchRefusal::SameModel);
    }
    match lifecycle.get_handle(&req.from_id).await {
        None => return Err(SwitchRefusal::NotLoaded),
        Some(handle) if handle != req.expected_handle => return Err(SwitchRefusal::StaleHandle),
        Some(_) => {}
    }
    if lifecycle.is_loaded(&req.to_id).await {
        return Err(SwitchRefusal::TargetLoaded);
    }
    lifecycle
        .source(req.expected_handle)
        .ok_or(SwitchRefusal::UnknownSource)
}

/// Validate A's recorded path and B's target without reading model bytes,
/// then estimate whether B fits once A's resident bytes are returned.
async fn preflight(
    lifecycle: &ModelLifecycle,
    prep: &Arc<dyn Preparer>,
    req: &ColdSwitchRequest,
    source: &ModelSource,
) -> Result<(), SwitchRefusal> {
    let (p, to, from) = (
        Arc::clone(prep),
        req.to_path.clone(),
        source.relative_path.clone(),
    );
    let facts = spawn_blocking(move || p.preflight(&from).and_then(|_| p.preflight(&to)))
        .await
        .map_err(|e| SwitchRefusal::Preflight(e.to_string()))?
        .map_err(|e| SwitchRefusal::Preflight(e.to_string()))?;
    if let Some(limits) = lifecycle.resource_limits() {
        let others = lifecycle.resident_bytes_excluding(&req.from_id).await as u64;
        let available = (limits.max_total_memory() as u64).saturating_sub(others);
        if facts.size_bytes > available {
            return Err(SwitchRefusal::OverBudget {
                needed: facts.size_bytes,
                available,
            });
        }
    }
    Ok(())
}

#[path = "transition_preparer.rs"]
mod preparer;

#[path = "transition_load.rs"]
mod load;
use load::{load_checked, prepare, LoadFailure};

#[path = "transition_recover.rs"]
mod recover;

#[cfg(test)]
#[path = "transition_test_support.rs"]
mod support;

#[cfg(test)]
#[path = "transition_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "transition_recovery_tests.rs"]
mod recovery_tests;
