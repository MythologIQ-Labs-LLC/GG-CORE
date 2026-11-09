//! Per-model memory reporting for admission (sidecar of `inference.rs`,
//! Section 4 Razor).

use super::InferenceEngine;

impl InferenceEngine {
    /// Per-request working memory a registered model reports, if known.
    pub async fn working_memory_estimate(&self, model_id: &str) -> Option<usize> {
        let models = self.models.read().await;
        models.get(model_id)?.working_memory_estimate()
    }

    /// Return the memory usage reported by a registered model, or None if not found.
    pub async fn model_memory_usage(&self, model_id: &str) -> Option<usize> {
        self.models
            .read()
            .await
            .get(model_id)
            .map(|m| m.memory_usage())
    }
}
