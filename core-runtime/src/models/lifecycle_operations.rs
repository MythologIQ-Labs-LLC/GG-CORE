//! Operator mutation authority and recorded model sources (#137 / #153).
//!
//! The authority orders operator mutations (daemon preload, IPC load and
//! unload, cold switch) so a multi-step transition is not interleaved. It is
//! **advisory**: embedded, FFI and Python callers of the plain
//! `ModelLifecycle::load`/`unload` primitives do not take it. Memory safety
//! does not depend on it; every path is bounded by the resident budget.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, OwnedMutexGuard};

use super::{resident, LifecycleError, ModelHandle, ModelLifecycle};

/// Where an operator-loaded model came from, bound to the bytes it loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSource {
    /// `base_path`-relative path the model was loaded from.
    pub(crate) relative_path: String,
    /// SHA-256 of the file, hashed before the load with its identity
    /// re-checked after (or the manifest's verified digest).
    pub(crate) sha256: String,
}

/// Held operator authority; dropping it releases the authority.
pub(crate) type OperationGuard = OwnedMutexGuard<()>;

/// Authority plus per-handle sources (one `ModelLifecycle` field).
#[derive(Default)]
pub(super) struct OpsState {
    authority: Arc<Mutex<()>>,
    sources: std::sync::Mutex<HashMap<u64, ModelSource>>,
}

impl OpsState {
    fn sources(&self) -> std::sync::MutexGuard<'_, HashMap<u64, ModelSource>> {
        self.sources.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forget a handle's source (called by every unload path).
    pub(super) fn forget(&self, handle: u64) {
        self.sources().remove(&handle);
    }
}

impl ModelLifecycle {
    /// Take the operator authority now, or `None` if another operation holds it.
    pub(crate) fn try_begin_operation(&self) -> Option<OperationGuard> {
        Arc::clone(&self.ops.authority).try_lock_owned().ok()
    }

    /// Take the operator authority, waiting at most `wait` (`None`: busy).
    pub(crate) async fn begin_operation_within(&self, wait: Duration) -> Option<OperationGuard> {
        let lock = Arc::clone(&self.ops.authority).lock_owned();
        tokio::time::timeout(wait, lock).await.ok()
    }

    /// Record the verified source of `handle`, only while that exact handle
    /// is still loaded (checked under the index lock), so a source can never
    /// attach to an instance a bypass path loaded in its place.
    pub(crate) async fn record_source(&self, handle: ModelHandle, source: ModelSource) -> bool {
        let index = self.index.read().await;
        let loaded = index.handle_to_id.contains_key(&handle.id());
        if loaded {
            self.ops.sources().insert(handle.id(), source);
        }
        loaded
    }

    /// The recorded source of a loaded handle, if an operator path loaded it.
    pub(crate) fn source(&self, handle: ModelHandle) -> Option<ModelSource> {
        self.ops.sources().get(&handle.id()).cloned()
    }

    /// Unload `model_id` only if it is still exactly `expected`; checked under
    /// the index write lock, so a concurrent replacement is never unloaded.
    pub(crate) async fn unload_expected(
        &self,
        model_id: &str,
        expected: ModelHandle,
    ) -> Result<ModelHandle, LifecycleError> {
        self.unload_matching(model_id, Some(expected)).await
    }

    /// Resident budget bytes `model_id` currently charges (0 if not loaded).
    pub(crate) async fn charged_bytes(&self, model_id: &str) -> usize {
        let index = self.index.read().await;
        index
            .residents
            .get(model_id)
            .map_or(0, |r| r.charged_bytes())
    }

    /// The same #219 all-locks-before-mutation transaction as ordinary unload,
    /// with an expected-handle check while the index is exclusively locked.
    pub(super) async fn unload_matching(
        &self,
        model_id: &str,
        expected: Option<ModelHandle>,
    ) -> Result<ModelHandle, LifecycleError> {
        let mut index = self.index.write().await;

        let handle = index
            .get_handle(model_id)
            .ok_or_else(|| LifecycleError::NotLoaded(model_id.into()))?;
        if expected.is_some_and(|expected| expected != handle) {
            return Err(LifecycleError::StaleHandle(model_id.into()));
        }

        // Query diagnostics before any mutation; this await is cancellable.
        let info = self.registry_info(handle).await;
        // Acquire both component writers before removing resident/index state.
        // A cancelled waiter leaves every store unchanged. Once acquired,
        // the registry, inference engine, and index commit with no await.
        let mut registry_writer = self.registry.registration_writer().await;
        let mut engine_writer = self.engine.registration_writer().await;
        let resident = index.residents.remove(model_id);
        let (done, watch) = resident::release_channel();
        if let Some(info) = info {
            self.releases.push(model_id.to_string(), info, watch);
        }
        engine_writer.remove(model_id);
        registry_writer.remove(handle);
        index.remove_by_id(model_id);
        self.ops.forget(handle.id());
        // Do not run a backend destructor while holding shared model writers.
        drop(engine_writer);
        drop(registry_writer);
        drop(index);
        // Budget returns only after native in-flight holders release the model.
        if let Some(entry) = resident {
            entry.release(done);
        }

        Ok(handle)
    }
}
