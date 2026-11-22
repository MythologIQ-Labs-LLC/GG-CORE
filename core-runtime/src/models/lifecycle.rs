//! Unified model lifecycle: single atomic load/unload pipeline.
//!
//! Ensures ModelRegistry and InferenceEngine are always in sync.
//! Provides O(1) bidirectional lookup between model_id and ModelHandle.

use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

use super::loader::ModelMetadata;
use super::registry::{ModelHandle, ModelRegistry};
use crate::engine::InferenceEngine;
use crate::engine::Model;

#[derive(Error, Debug)]
pub enum LifecycleError {
    #[error("Model already loaded: {0}")]
    AlreadyLoaded(String),

    #[error("Model not loaded: {0}")]
    NotLoaded(String),

    #[error("Load failed: {0}")]
    LoadFailed(String),

    /// The model's resident bytes do not fit the total-memory budget
    /// (`GG_CORE_MAX_TOTAL_MEMORY`, also the Windows Job Object cap).
    #[error("Insufficient memory budget: {0} (raise GG_CORE_MAX_TOTAL_MEMORY or unload a model)")]
    ResourceExhausted(String),

    /// The model id was replaced after the caller observed its old handle.
    #[error("Stale model handle: {0}")]
    StaleHandle(String),
}

#[path = "lifecycle_index.rs"]
mod lookup_index;
use lookup_index::LookupIndex;

/// Unified model lifecycle coordinator.
///
/// All model loads and unloads go through this to keep
/// ModelRegistry and InferenceEngine atomically in sync.
pub struct ModelLifecycle {
    registry: Arc<ModelRegistry>,
    engine: Arc<InferenceEngine>,
    index: Arc<RwLock<LookupIndex>>,
    /// Resident-weight accounting (None: unlimited, embedded/test use).
    limits: Option<crate::memory::ResourceLimits>,
    /// Unloaded instances whose memory is still held.
    releases: release::ReleaseTracker,
    /// Operator mutation authority and authenticated model sources.
    ops: operations::OpsState,
}

impl ModelLifecycle {
    pub fn new(registry: Arc<ModelRegistry>, engine: Arc<InferenceEngine>) -> Self {
        Self {
            registry,
            engine,
            index: Arc::new(RwLock::new(LookupIndex::new())),
            limits: None,
            releases: release::ReleaseTracker::default(),
            ops: operations::OpsState::default(),
        }
    }

    /// Account every loaded model's resident bytes against `limits`.
    pub fn with_resource_limits(mut self, limits: crate::memory::ResourceLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// The resource limits loads are admitted against, if any.
    pub fn resource_limits(&self) -> Option<&crate::memory::ResourceLimits> {
        self.limits.as_ref()
    }

    /// Atomically load a model into both registry and engine.
    ///
    /// Holds write lock for the entire duration to prevent TOCTOU
    /// races on concurrent loads of the same model_id.
    pub async fn load(
        &self,
        model_id: String,
        metadata: ModelMetadata,
        model: Arc<dyn Model>,
    ) -> Result<ModelHandle, LifecycleError> {
        self.load_reserved(model_id, metadata, model, None).await
    }

    /// [`Self::load`] that adopts a reservation taken before backend
    /// allocation (re-sized to the model's reported footprint). On any
    /// refusal the model is dropped before its reservation is released.
    pub async fn load_reserved(
        &self,
        model_id: String,
        metadata: ModelMetadata,
        model: Arc<dyn Model>,
        reservation: Option<crate::memory::ResidentGuard>,
    ) -> Result<ModelHandle, LifecycleError> {
        // Own backend and reservation *before* the first suspension.
        // An aborted registration waiting on the index lock must not drop
        // the guard separately while a detached native task holds the model.
        let mut entry = resident::Resident::new(model, reservation);
        let memory_usage = entry.memory_usage();
        let mut index = self.index.write().await;
        let admitted = if index.contains_id(&model_id) {
            Err(LifecycleError::AlreadyLoaded(model_id.clone()))
        } else {
            resident::admit(self.limits.as_ref(), entry.reservation_mut(), memory_usage)
        };
        if let Err(e) = admitted {
            // A backend destructor may block or panic. Never execute it while
            // holding the lifecycle's exclusive index lock: unrelated loads,
            // unloads and lookups must still make progress (#219).
            drop(index);
            // Resident releases only after the backend's last Arc is gone,
            // and retains budget if native destruction panics (#219).
            drop(entry);
            return Err(e);
        }
        // Acquire every fallible-by-cancellation lock before the first
        // mutation. Once both permits exist, registry, engine, and the
        // lifecycle index commit synchronously with no intervening await.
        // Dropping this future while waiting cannot publish a partial model.
        let mut registry_writer = self.registry.registration_writer().await;
        let mut engine_writer = self.engine.registration_writer().await;
        let handle = registry_writer.insert(metadata, memory_usage);
        engine_writer.insert(model_id.clone(), entry.clone_model());
        index.residents.insert(model_id.clone(), entry);
        index.insert(model_id, handle);

        Ok(handle)
    }

    /// Atomically unload a model from both registry and engine.
    ///
    /// Holds write lock for the entire duration to prevent races. The
    /// instance is recorded as releasing *before* it leaves the registry, so
    /// a concurrent listing never misses memory that is still held.
    pub async fn unload(&self, model_id: &str) -> Result<ModelHandle, LifecycleError> {
        self.unload_matching(model_id, None).await
    }

    async fn registry_info(&self, handle: ModelHandle) -> Option<super::LoadedModelInfo> {
        let models = self.registry.list_models().await;
        models.into_iter().find(|m| m.handle_id == handle.id())
    }

    /// O(1) lookup: model_id -> ModelHandle.
    pub async fn get_handle(&self, model_id: &str) -> Option<ModelHandle> {
        self.index.read().await.get_handle(model_id)
    }

    /// O(1) lookup: handle_id -> model_id.
    pub async fn get_model_id(&self, handle_id: u64) -> Option<String> {
        self.index.read().await.get_id(handle_id).map(String::from)
    }

    /// Check if a model is loaded.
    pub async fn is_loaded(&self, model_id: &str) -> bool {
        self.index.read().await.contains_id(model_id)
    }

    /// Number of loaded models.
    pub async fn count(&self) -> usize {
        self.index.read().await.id_to_handle.len()
    }

    /// Validate that a handle exists in both the lookup index and the registry.
    ///
    /// Returns `false` if the handle is stale (present in one but not the other,
    /// or absent from both).
    pub async fn validate_handle(&self, handle: &ModelHandle) -> bool {
        let in_index = self
            .index
            .read()
            .await
            .handle_to_id
            .contains_key(&handle.id());
        let in_registry = self.registry.contains(*handle).await;
        in_index && in_registry
    }
}

/// Retire an abandoned prepared backend through the same panic-safe native
/// model/reservation boundary used for unload and failed registration (#219).
/// The model may still be held by a detached native task.
pub(super) fn release_prepared_reservation(
    model: Arc<dyn Model>,
    guard: crate::memory::ResidentGuard,
) {
    let (done, _unused_watch) = resident::release_channel();
    resident::Resident::new(model, Some(guard)).release(done);
}

#[path = "lifecycle_resident.rs"]
mod resident;

#[path = "lifecycle_release.rs"]
mod release;
pub use release::MAX_RELEASE_WAIT;

#[path = "lifecycle_operations.rs"]
mod operations;
pub(crate) use operations::{ModelSource, OperationGuard};

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lifecycle_resident_tests.rs"]
mod resident_tests;

#[cfg(test)]
#[path = "lifecycle_refusal_drop_tests.rs"]
mod refusal_drop_tests;

#[cfg(test)]
#[path = "lifecycle_release_tests.rs"]
mod release_tests;

#[cfg(test)]
#[path = "lifecycle_transition_tests.rs"]
mod transition_tests;
