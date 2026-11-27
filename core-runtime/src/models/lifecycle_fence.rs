//! Drain-before-unload support for the cold switch (#153 option 2, S9).
//!
//! The lifecycle owns exactly two references to a loaded model: the engine's
//! store entry and its own [`super::resident::Resident`]. Every reference
//! above that is an in-flight lease (a worker, a native stream thread, the
//! optional speculative draft). Nothing here mutates the registry, the
//! budget or the index; it only closes a model to new admissions and counts
//! what is still out.

use std::time::Duration;

use super::{LifecycleError, ModelHandle, ModelLifecycle, MAX_RELEASE_WAIT};
use crate::engine::AdmissionFence;

/// The engine store entry and the lifecycle's `Resident`.
const OWN_REFERENCES: usize = 2;

/// How often a drain re-reads the lease count (no lock is held between reads).
const POLL: Duration = Duration::from_millis(10);

impl ModelLifecycle {
    /// Close `model_id` to new admissions while it is exactly `expected`
    /// (checked under the index lock), returning the fence and the leases
    /// outstanding at the instant it was raised. A does not leave the
    /// registry; dropping the fence reopens it unchanged.
    pub(crate) async fn fence_admissions(
        &self,
        model_id: &str,
        expected: ModelHandle,
    ) -> Result<(AdmissionFence, usize), LifecycleError> {
        let index = self.index.read().await;
        match index.get_handle(model_id) {
            None => return Err(LifecycleError::NotLoaded(model_id.into())),
            Some(handle) if handle != expected => {
                return Err(LifecycleError::StaleHandle(model_id.into()))
            }
            Some(_) => {}
        }
        let (fence, count) = self
            .engine
            .fence_admissions(model_id)
            .await
            .ok_or_else(|| LifecycleError::NotLoaded(model_id.into()))?;
        Ok((fence, count.saturating_sub(OWN_REFERENCES)))
    }

    /// In-flight leases on `model_id` now (`None`: not registered).
    pub(crate) async fn leases(&self, model_id: &str) -> Option<usize> {
        let count = self.engine.strong_count(model_id).await?;
        Some(count.saturating_sub(OWN_REFERENCES))
    }

    /// Wait up to `wait` (capped at [`MAX_RELEASE_WAIT`]) for every lease on
    /// `model_id` to end. Returns the leases still outstanding: 0 means
    /// drained. Meaningful only while the model is fenced, otherwise new
    /// leases may appear between reads.
    pub(crate) async fn wait_drained(&self, model_id: &str, wait: Duration) -> usize {
        let deadline = tokio::time::Instant::now() + wait.min(MAX_RELEASE_WAIT);
        loop {
            let leases = self.leases(model_id).await.unwrap_or(0);
            let now = tokio::time::Instant::now();
            if leases == 0 || now >= deadline {
                return leases;
            }
            tokio::time::sleep(deadline.saturating_duration_since(now).min(POLL)).await;
        }
    }
}
