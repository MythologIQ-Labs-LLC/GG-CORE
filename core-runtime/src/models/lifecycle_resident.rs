//! Resident-budget bookkeeping for `ModelLifecycle` (Section 4 Razor sidecar).
//!
//! A loaded model's budget is held by a [`Resident`]: the lifecycle's own
//! `Arc` to the model plus its [`ResidentGuard`]. On unload the budget is
//! returned only when the model's memory can actually be freed: if an
//! in-flight request still holds the model, release is deferred until that
//! request drops its handle (requests are deadline-bounded), so a following
//! load can never be admitted against bytes that are still allocated.

use std::sync::Arc;
use std::time::Duration;

use super::LifecycleError;
use crate::engine::Model;
use crate::memory::{ResidentGuard, ResourceLimits};

/// Poll interval while an unloaded model is still referenced by a request.
const RELEASE_POLL: Duration = Duration::from_millis(25);

/// A registered model's budget: the lifecycle's model handle and its guard.
/// Field order: `model` drops before `guard` (memory freed, then budget).
pub(super) struct Resident {
    model: Arc<dyn Model>,
    guard: ResidentGuard,
}

/// Charge `memory` resident bytes into `slot`: re-size a pre-load
/// reservation in place, or take a fresh one. On refusal `slot` keeps its
/// prior reservation so the caller controls drop order. `None` limits and
/// no reservation means unaccounted (embedded/test use).
pub(super) fn admit(
    limits: Option<&ResourceLimits>,
    slot: &mut Option<ResidentGuard>,
    memory: usize,
) -> Result<(), LifecycleError> {
    let exhausted =
        |e: crate::engine::InferenceError| LifecycleError::ResourceExhausted(e.to_string());
    match (limits, slot.as_mut()) {
        (_, Some(guard)) => guard.resize(memory).map_err(exhausted),
        (Some(limits), None) => {
            *slot = Some(limits.reserve_resident(memory).map_err(exhausted)?);
            Ok(())
        }
        (None, None) => Ok(()),
    }
}

impl Resident {
    pub(super) fn new(model: Arc<dyn Model>, guard: ResidentGuard) -> Self {
        Self { model, guard }
    }

    /// Return the budget once no request holds the model any more. Must be
    /// called after the engine dropped its own reference.
    pub(super) fn release(self) {
        if Arc::strong_count(&self.model) == 1 {
            return; // dropped here: model memory freed, then budget released
        }
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn(self.release_when_idle());
            }
            // No runtime: keep the budget charged for the rest of the process
            // rather than release bytes that are still allocated (fail closed).
            Err(_) => std::mem::forget(self.guard),
        }
    }

    async fn release_when_idle(self) {
        while Arc::strong_count(&self.model) > 1 {
            tokio::time::sleep(RELEASE_POLL).await;
        }
    }
}
