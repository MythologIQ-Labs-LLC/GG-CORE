//! Unified model lifecycle: single atomic load/unload pipeline.
//!
//! Ensures ModelRegistry and InferenceEngine are always in sync.
//! Provides O(1) bidirectional lookup between model_id and ModelHandle.

use std::collections::HashMap;
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
}

/// Bidirectional O(1) lookup between model_id and ModelHandle.
struct LookupIndex {
    id_to_handle: HashMap<String, ModelHandle>,
    handle_to_id: HashMap<u64, String>,
    /// Resident-weight reservation per loaded model, released on unload.
    residents: HashMap<String, crate::memory::ResidentGuard>,
}

impl LookupIndex {
    fn new() -> Self {
        Self {
            id_to_handle: HashMap::new(),
            handle_to_id: HashMap::new(),
            residents: HashMap::new(),
        }
    }

    fn insert(&mut self, model_id: String, handle: ModelHandle) {
        self.handle_to_id.insert(handle.id(), model_id.clone());
        self.id_to_handle.insert(model_id, handle);
    }

    fn remove_by_id(&mut self, model_id: &str) -> Option<ModelHandle> {
        if let Some(handle) = self.id_to_handle.remove(model_id) {
            self.handle_to_id.remove(&handle.id());
            return Some(handle);
        }
        None
    }

    fn get_handle(&self, model_id: &str) -> Option<ModelHandle> {
        self.id_to_handle.get(model_id).copied()
    }

    fn get_id(&self, handle_id: u64) -> Option<&str> {
        self.handle_to_id.get(&handle_id).map(|s| s.as_str())
    }

    fn contains_id(&self, model_id: &str) -> bool {
        self.id_to_handle.contains_key(model_id)
    }
}

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
}

impl ModelLifecycle {
    pub fn new(registry: Arc<ModelRegistry>, engine: Arc<InferenceEngine>) -> Self {
        Self {
            registry,
            engine,
            index: Arc::new(RwLock::new(LookupIndex::new())),
            limits: None,
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
        let mut index = self.index.write().await;

        if index.contains_id(&model_id) {
            return Err(LifecycleError::AlreadyLoaded(model_id));
        }

        let memory = model.memory_usage();
        // Reserve resident weights before registering: a model that does not
        // fit is refused explicitly and never becomes visible.
        let resident = match &self.limits {
            Some(limits) => Some(
                limits
                    .reserve_resident(memory)
                    .map_err(|e| LifecycleError::ResourceExhausted(e.to_string()))?,
            ),
            None => None,
        };
        let handle = self.registry.register(metadata, memory).await;
        self.engine
            .register_model(model_id.clone(), handle, model)
            .await;
        if let Some(guard) = resident {
            index.residents.insert(model_id.clone(), guard);
        }
        index.insert(model_id, handle);

        Ok(handle)
    }

    /// Atomically unload a model from both registry and engine.
    ///
    /// Holds write lock for the entire duration to prevent races.
    pub async fn unload(&self, model_id: &str) -> Result<ModelHandle, LifecycleError> {
        let mut index = self.index.write().await;

        let handle = index
            .get_handle(model_id)
            .ok_or_else(|| LifecycleError::NotLoaded(model_id.into()))?;

        self.engine.unregister_model(model_id).await;
        self.registry.unregister(handle).await;
        index.remove_by_id(model_id);
        // Dropping the guard releases the model's resident reservation.
        index.residents.remove(model_id);

        Ok(handle)
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

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lifecycle_resident_tests.rs"]
mod resident_tests;
