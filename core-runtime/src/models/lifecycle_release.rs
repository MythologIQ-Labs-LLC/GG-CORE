//! Unloaded-but-not-yet-released model instances (Section 4 Razor sidecar).
//!
//! `ModelLifecycle::unload` pushes an entry here *before* the model leaves
//! the registry, so a listing that reads the registry first and this tracker
//! second (deduplicating by the never-reused `handle_id`) always shows every
//! instance whose memory is still held, exactly once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::resident::ReleaseWatch;
use super::ModelLifecycle;
use crate::models::registry::{LoadedModelInfo, LoadedModelState};

/// Longest wait a caller may request for a release (IPC clamps to this).
pub const MAX_RELEASE_WAIT: Duration = Duration::from_secs(120);

struct Releasing {
    model_id: String,
    info: LoadedModelInfo,
    watch: ReleaseWatch,
}

/// Instances unloaded from the registry whose memory is still held.
#[derive(Clone, Default)]
pub(super) struct ReleaseTracker {
    entries: Arc<Mutex<Vec<Releasing>>>,
}

impl ReleaseTracker {
    pub(super) fn push(&self, model_id: String, info: LoadedModelInfo, watch: ReleaseWatch) {
        let entry = Releasing {
            model_id,
            info,
            watch,
        };
        self.lock().push(entry);
    }

    /// Drop released entries; return clones of the pending ones' data.
    fn pending(&self) -> Vec<(String, LoadedModelInfo, ReleaseWatch)> {
        let mut entries = self.lock();
        entries.retain(|e| !e.watch.released());
        entries
            .iter()
            .map(|e| (e.model_id.clone(), e.info.clone(), e.watch.clone()))
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Releasing>> {
        // The lock guards only Vec bookkeeping; a poisoned lock still holds
        // consistent data, so keep serving rather than hide pending entries.
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl ModelLifecycle {
    /// Unloaded instances whose memory is still held by in-flight requests,
    /// each with its original `handle_id` and `state = Unloading`.
    pub fn releasing_models(&self) -> Vec<LoadedModelInfo> {
        let mut out: Vec<LoadedModelInfo> = self
            .releases
            .pending()
            .into_iter()
            .map(|(_, info, _)| info)
            .collect();
        for info in &mut out {
            info.state = LoadedModelState::Unloading;
        }
        out
    }

    /// Wait up to `wait` (capped at [`MAX_RELEASE_WAIT`]) until no unloaded
    /// instance of `model_id` is still held. Returns `(released, pending)`:
    /// `released` is true when none remain, including an id that was never
    /// loaded. No lifecycle or tracker lock is held while waiting.
    pub async fn wait_released(&self, model_id: &str, wait: Duration) -> (bool, usize) {
        let deadline = tokio::time::Instant::now() + wait.min(MAX_RELEASE_WAIT);
        let watches: Vec<ReleaseWatch> = self
            .releases
            .pending()
            .into_iter()
            .filter(|(id, _, _)| id == model_id)
            .map(|(_, _, watch)| watch)
            .collect();
        for watch in watches {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            watch.wait(left).await;
        }
        let pending = self.pending_count(model_id);
        (pending == 0, pending)
    }

    fn pending_count(&self, model_id: &str) -> usize {
        let pending = self.releases.pending();
        pending.iter().filter(|(id, _, _)| id == model_id).count()
    }
}
