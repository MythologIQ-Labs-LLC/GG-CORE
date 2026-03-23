//! Model pool for instant switching between pre-loaded models.
//!
//! Maintains multiple models in memory to enable seamless tier transitions
//! without load-time latency.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use super::registry::{ModelHandle, ModelRegistry};
pub use super::pool_types::*;

/// Pooled model entry with usage tracking.
#[derive(Debug)]
struct PooledModel {
    handle: ModelHandle,
    #[allow(dead_code)]
    model_id: String,
    tier: ModelTier,
    memory_bytes: usize,
    #[allow(dead_code)]
    loaded_at: Instant,
    last_used: Instant,
    use_count: u64,
    warmup_complete: bool,
}

impl PooledModel {
    /// Calculate eviction score (lower = evict first).
    fn eviction_score(&self) -> u64 {
        let tier_weight = (self.tier as u64) * 1_000_000;
        let recency_weight = self.last_used.elapsed().as_secs();
        let usage_weight = self.use_count.min(1000);
        tier_weight + usage_weight - recency_weight.min(999)
    }
}

/// Model pool for instant tier switching.
pub struct ModelPool {
    config: PoolConfig,
    registry: Arc<ModelRegistry>,
    models: Arc<RwLock<HashMap<String, PooledModel>>>,
    active_model: Arc<RwLock<Option<String>>>,
    metrics: Arc<RwLock<PoolMetrics>>,
}

impl ModelPool {
    pub fn new(config: PoolConfig, registry: Arc<ModelRegistry>) -> Self {
        Self {
            config,
            registry,
            models: Arc::new(RwLock::new(HashMap::new())),
            active_model: Arc::new(RwLock::new(None)),
            metrics: Arc::new(RwLock::new(PoolMetrics::default())),
        }
    }

    /// Add a model to the pool (preload without activating).
    pub async fn preload(
        &self,
        model_id: String,
        handle: ModelHandle,
        tier: ModelTier,
        memory_bytes: usize,
    ) -> Result<(), PoolError> {
        let mut models = self.models.write().await;

        if models.contains_key(&model_id) {
            return Err(PoolError::AlreadyLoaded(model_id));
        }

        if models.len() >= self.config.max_models {
            drop(models);
            self.evict_one().await?;
            models = self.models.write().await;
        }

        let current_memory: usize = models.values().map(|m| m.memory_bytes).sum();
        if current_memory + memory_bytes > self.config.max_memory_bytes {
            drop(models);
            self.evict_for_memory(memory_bytes).await?;
            models = self.models.write().await;
        }

        let now = Instant::now();
        models.insert(
            model_id.clone(),
            PooledModel {
                handle, model_id, tier, memory_bytes,
                loaded_at: now, last_used: now,
                use_count: 0, warmup_complete: false,
            },
        );

        Ok(())
    }

    /// Switch to a model in the pool (instant if preloaded).
    pub async fn switch_to(&self, model_id: &str) -> Result<SwitchResult, PoolError> {
        let start = Instant::now();

        let mut models = self.models.write().await;
        let model = models
            .get_mut(model_id)
            .ok_or_else(|| PoolError::ModelNotFound(model_id.to_string()))?;

        model.last_used = Instant::now();
        model.use_count += 1;
        let handle = model.handle;
        let was_warmed = model.warmup_complete;
        drop(models);

        *self.active_model.write().await = Some(model_id.to_string());
        let switch_latency = start.elapsed();

        crate::telemetry::record_model_switch_latency(
            model_id,
            switch_latency.as_secs_f64(),
        );

        let mut metrics = self.metrics.write().await;
        metrics.pool_hits += 1;
        let total = metrics.pool_hits;
        metrics.avg_switch_latency_ns =
            (metrics.avg_switch_latency_ns * (total - 1) + switch_latency.as_nanos() as u64) / total;

        Ok(SwitchResult { handle, switch_latency, was_preloaded: true, was_warmed })
    }

    /// Mark a model as warmed up (after running warmup inference).
    pub async fn mark_warmed(&self, model_id: &str) {
        if let Some(model) = self.models.write().await.get_mut(model_id) {
            model.warmup_complete = true;
            self.metrics.write().await.warmups_completed += 1;
        }
    }

    /// Evict lowest-priority model from pool.
    async fn evict_one(&self) -> Result<String, PoolError> {
        let mut models = self.models.write().await;
        let active = self.active_model.read().await.clone();

        let evict_id = models
            .iter()
            .filter(|(id, _)| active.as_ref() != Some(*id))
            .min_by_key(|(_, m)| m.eviction_score())
            .map(|(id, _)| id.clone());

        if let Some(id) = evict_id {
            let model = models.remove(&id).unwrap();
            self.registry.unregister(model.handle).await;
            self.metrics.write().await.evictions += 1;
            Ok(id)
        } else {
            Err(PoolError::EvictionFailed)
        }
    }

    /// Evict models until we have enough memory.
    async fn evict_for_memory(&self, needed_bytes: usize) -> Result<(), PoolError> {
        loop {
            let current: usize = self.models.read().await.values().map(|m| m.memory_bytes).sum();
            if current + needed_bytes <= self.config.max_memory_bytes {
                return Ok(());
            }
            self.evict_one().await?;
        }
    }

    /// Get current pool status.
    pub async fn status(&self) -> PoolStatus {
        let models = self.models.read().await;
        let active = self.active_model.read().await.clone();
        let metrics = self.metrics.read().await.clone();
        PoolStatus {
            model_count: models.len(),
            total_memory_bytes: models.values().map(|m| m.memory_bytes).sum(),
            active_model: active,
            loaded_models: models.keys().cloned().collect(),
            metrics,
        }
    }

    /// Check if a model is in the pool.
    pub async fn contains(&self, model_id: &str) -> bool {
        self.models.read().await.contains_key(model_id)
    }

    /// Get the active model ID.
    pub async fn active(&self) -> Option<String> {
        self.active_model.read().await.clone()
    }

    /// Remove a model from the pool.
    pub async fn remove(&self, model_id: &str) -> Option<ModelHandle> {
        let mut models = self.models.write().await;
        if let Some(model) = models.remove(model_id) {
            let mut active = self.active_model.write().await;
            if active.as_ref() == Some(&model_id.to_string()) {
                *active = None;
            }
            Some(model.handle)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
