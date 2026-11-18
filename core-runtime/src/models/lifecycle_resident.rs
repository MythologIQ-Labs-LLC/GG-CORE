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

use std::sync::{mpsc, Arc, OnceLock};
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

/// A pending model release must outlive Tokio tasks and the runtime itself:
/// detached native inference can still hold its Arc after the async worker is
/// aborted. One process-wide standard thread polls all pending residents,
/// rather than spawning an unbounded thread or cancellation-unsafe async task
/// for each unload (#219).
static RELEASE_SUPERVISOR: OnceLock<Option<mpsc::Sender<PendingRelease>>> = OnceLock::new();

struct PendingRelease {
    resident: Option<Resident>,
    done: watch::Sender<bool>,
}

impl PendingRelease {
    fn ready(&self) -> bool {
        self.resident
            .as_ref()
            .is_some_and(|resident| Arc::strong_count(&resident.model) == 1)
    }

    fn finish(mut self) {
        // Model destructor completes before the resident budget returns.
        if let Some(resident) = self.resident.take() {
            drop(resident);
            let _ = self.done.send(true);
        }
    }
}

impl Drop for PendingRelease {
    fn drop(&mut self) {
        let Some(resident) = self.resident.take() else {
            return;
        };
        if Arc::strong_count(&resident.model) > 1 {
            // Catastrophic supervisor shutdown/panic: never credit memory
            // while a detached native worker still holds the model. Normal
            // release has no leak; this is an explicit fail-closed fallback.
            let Resident { model, guard } = resident;
            std::mem::forget(guard);
            drop(model);
        } else {
            drop(resident);
        }
    }
}

fn release_supervisor() -> Option<&'static mpsc::Sender<PendingRelease>> {
    RELEASE_SUPERVISOR
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            match std::thread::Builder::new()
                .name("gg-core-release".into())
                .spawn(move || supervise_releases(receiver))
            {
                Ok(thread) => {
                    drop(thread); // detach from Tokio and caller cancellation
                    Some(sender)
                }
                Err(error) => {
                    tracing::error!(
                        target: "gg_core::lifecycle",
                        %error,
                        "model release supervisor unavailable; retain budget"
                    );
                    None
                }
            }
        })
        .as_ref()
}

fn supervise_releases(receiver: mpsc::Receiver<PendingRelease>) {
    let mut pending: Vec<PendingRelease> = Vec::new();
    loop {
        if pending.is_empty() {
            match receiver.recv() {
                Ok(release) => pending.push(release),
                Err(_) => break,
            }
        } else {
            match receiver.recv_timeout(RELEASE_POLL) {
                Ok(release) => pending.push(release),
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
            }
        }

        let mut index = 0;
        while index < pending.len() {
            if pending[index].ready() {
                pending.swap_remove(index).finish();
            } else {
                index += 1;
            }
        }
    }
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
        // The supervisor holds this entire Resident independently of an
        // aborted waiter or dropped Tokio runtime. A failed spawn/send drops
        // PendingRelease through its fail-closed Drop guard.
        let pending = PendingRelease {
            resident: Some(self),
            done,
        };
        match release_supervisor() {
            Some(sender) => {
                if let Err(error) = sender.send(pending) {
                    tracing::error!(
                        target: "gg_core::lifecycle",
                        "model release supervisor disconnected; retain budget"
                    );
                    drop(error.0);
                }
            }
            None => drop(pending),
        }
    }
}
