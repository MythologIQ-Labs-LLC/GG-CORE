//! Resident-budget bookkeeping for `ModelLifecycle` (Section 4 Razor sidecar).
//!
//! Every loaded model is held by a [`Resident`]: the lifecycle's own `Arc` to
//! the model plus its [`ResidentGuard`] (absent when no limits are set). On
//! unload the budget is returned only when the model's memory can actually be
//! freed: if an in-flight request still holds the model, release is deferred
//! until that request drops its handle, so a following load can never be
//! admitted against bytes that are still allocated.
//!
//! "Released" means: the last `Arc<dyn Model>` was dropped (backend
//! destructors ran) and then the guard returned the budget. It is *not* a
//! claim that the OS reclaimed every page (see llama-cpp-rs#1194).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use super::LifecycleError;
use crate::engine::Model;
use crate::memory::{ResidentGuard, ResourceLimits};

/// Poll interval while an unloaded model is still referenced by a request.
const RELEASE_POLL: Duration = Duration::from_millis(25);

/// A registered model's budget: the lifecycle's model handle and its guard.
/// Field order: `model` drops before `guard` (memory freed, then budget).
pub(super) struct Resident {
    model: Arc<dyn Model>,
    guard: Option<ResidentGuard>,
}

/// Observes whether an unloaded instance has been released. A watch whose
/// sender was dropped without signalling (runtime shutdown, or no runtime)
/// reads as *not* released, forever: fail closed.
#[derive(Clone)]
pub(super) struct ReleaseWatch(watch::Receiver<bool>);

impl ReleaseWatch {
    /// True only once the release has actually completed.
    pub(super) fn released(&self) -> bool {
        *self.0.borrow()
    }

    /// Wait up to `wait` for the release. Returns at once with `false` if
    /// the release can no longer happen (sender dropped unsignalled).
    pub(super) async fn wait(mut self, wait: Duration) -> bool {
        let done = tokio::time::timeout(wait, self.0.wait_for(|released| *released)).await;
        matches!(done, Ok(Ok(_)))
    }
}

/// An unsignalled watch, fulfilled later by [`Resident::release`].
pub(super) fn release_channel() -> (watch::Sender<bool>, ReleaseWatch) {
    let (tx, rx) = watch::channel(false);
    (tx, ReleaseWatch(rx))
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
    pub(super) fn new(model: Arc<dyn Model>, guard: Option<ResidentGuard>) -> Self {
        Self { model, guard }
    }

    /// Return the budget once no request holds the model any more, then
    /// signal `done`. Must be called after the engine dropped its own
    /// reference.
    pub(super) fn release(self, done: watch::Sender<bool>) {
        if Arc::strong_count(&self.model) == 1 {
            drop(self); // model memory freed, then budget released
            let _ = done.send(true);
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn(self.release_when_idle(done));
            }
            // No runtime: keep the budget charged for the rest of the process
            // rather than release bytes that are still allocated (fail closed).
            // `done` drops unsignalled, so the instance never reads released.
            Err(_) => std::mem::forget(self.guard),
        }
    }

    async fn release_when_idle(self, done: watch::Sender<bool>) {
        while Arc::strong_count(&self.model) > 1 {
            tokio::time::sleep(RELEASE_POLL).await;
        }
        drop(self);
        let _ = done.send(true);
    }
}
