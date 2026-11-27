//! Model ID/handle lookup and resident ownership index.
//! Extracted from lifecycle.rs without changing lock or admission behavior.

use std::collections::HashMap;

use super::resident::Resident;
use crate::models::registry::ModelHandle;

/// Bidirectional O(1) lookup between model_id and ModelHandle.
pub(super) struct LookupIndex {
    pub(super) id_to_handle: HashMap<String, ModelHandle>,
    pub(super) handle_to_id: HashMap<u64, String>,
    /// Resident-weight budget per loaded model, released on unload once no
    /// request still holds the model.
    pub(super) residents: HashMap<String, Resident>,
}

impl LookupIndex {
    pub(super) fn new() -> Self {
        Self {
            id_to_handle: HashMap::new(),
            handle_to_id: HashMap::new(),
            residents: HashMap::new(),
        }
    }

    pub(super) fn insert(&mut self, model_id: String, handle: ModelHandle) {
        self.handle_to_id.insert(handle.id(), model_id.clone());
        self.id_to_handle.insert(model_id, handle);
    }

    pub(super) fn remove_by_id(&mut self, model_id: &str) -> Option<ModelHandle> {
        if let Some(handle) = self.id_to_handle.remove(model_id) {
            self.handle_to_id.remove(&handle.id());
            return Some(handle);
        }
        None
    }

    pub(super) fn get_handle(&self, model_id: &str) -> Option<ModelHandle> {
        self.id_to_handle.get(model_id).copied()
    }

    pub(super) fn get_id(&self, handle_id: u64) -> Option<&str> {
        self.handle_to_id.get(&handle_id).map(|s| s.as_str())
    }

    pub(super) fn contains_id(&self, model_id: &str) -> bool {
        self.id_to_handle.contains_key(model_id)
    }
}
