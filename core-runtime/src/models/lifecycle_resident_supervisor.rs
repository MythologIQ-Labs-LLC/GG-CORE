//! Runtime-independent native model release supervisor (#219).
//! Fail-closed budget accounting survives lost Tokio runtimes and aborts.

use std::sync::{mpsc, OnceLock};
use std::time::Duration;

use tokio::sync::watch;

use super::{PendingRelease, Resident};

/// Poll interval while an unloaded model is still referenced by a request.
const RELEASE_POLL: Duration = Duration::from_millis(25);

/// A pending model release must outlive Tokio tasks and the runtime itself:
/// detached native inference can still hold its Arc after the async worker is
/// aborted. One process-wide standard thread polls all pending residents,
/// rather than spawning an unbounded thread or cancellation-unsafe async task
/// for each unload (#219).
static RELEASE_SUPERVISOR: OnceLock<Option<mpsc::Sender<PendingRelease>>> = OnceLock::new();

pub(super) fn enqueue_release(resident: Resident, done: watch::Sender<bool>) {
    // Failed spawn/send must not free still-live model memory; PendingRelease
    // drops through the explicit fail-closed budget-retention path.
    let pending = PendingRelease {
        resident: Some(resident),
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

