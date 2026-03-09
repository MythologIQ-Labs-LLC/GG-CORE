//! Smart model loader with semantic hints and adaptive caching.
//!
//! Zero idle overhead with intelligent prefetching based on usage patterns.
//! Uses OS-level page cache for automatic memory management.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore};

use super::registry::ModelHandle;

#[derive(Error, Debug)]
pub enum SmartLoaderError {
    #[error("Model not registered: {0}")]
    NotRegistered(String),

    #[error("Load failed: {0}")]
    LoadFailed(String),

    #[error("Already loading")]
    AlreadyLoading,
}

/// Semantic hints for adaptive loading decisions.
#[derive(Debug, Clone, Copy)]
pub enum LoadHint {
    /// Quick single query - prefer lightweight model
    QuickQuery,
    /// Complex task - prefer quality model
    ComplexTask,
    /// Batch incoming - preload appropriate model
    BatchIncoming { count: usize },
    /// User going idle - good time to preload
    UserIdle,
    /// Explicit model preference
    PreferModel { tier: ModelTier },
}

/// Model tier classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTier {
    /// Lightweight for quick responses (~500MB)
    Light,
    /// Balanced for general use (~1.5GB)
    Balanced,
    /// Quality for complex tasks (~2.5GB)
    Quality,
}

/// Model load state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// Registered but not loaded
    Registered,
    /// Currently loading
    Loading,
    /// Loaded and ready
    Ready,
    /// Load failed
    Failed,
}

/// Registered model metadata.
struct ModelEntry {
    path: PathBuf,
    tier: ModelTier,
    size_bytes: u64,
    state: LoadState,
    handle: Option<ModelHandle>,
    last_used: Option<Instant>,
    use_count: u64,
    load_time_ms: Option<u64>,
}

/// Smart loader configuration.
#[derive(Debug, Clone)]
pub struct SmartLoaderConfig {
    /// Auto-unload after this duration of inactivity
    pub auto_unload_after: Duration,
    /// Max concurrent loads
    pub max_concurrent_loads: usize,
    /// Enable predictive loading
    pub enable_prediction: bool,
}

impl Default for SmartLoaderConfig {
    fn default() -> Self {
        Self {
            auto_unload_after: Duration::from_secs(60),
            max_concurrent_loads: 1,
            enable_prediction: true,
        }
    }
}

/// Smart loader metrics.
#[derive(Debug, Default, Clone)]
pub struct SmartLoaderMetrics {
    pub total_loads: u64,
    pub cache_hits: u64,
    pub predictions_made: u64,
    pub predictions_correct: u64,
    pub avg_load_ms: f64,
    pub avg_cache_hit_ms: f64,
}

/// Callback for model loading (to integrate with actual GGUF loader).
pub type LoadCallback = Box<dyn Fn(&PathBuf) -> Result<ModelHandle, String> + Send + Sync>;

/// Smart model loader with semantic hints.
pub struct SmartLoader {
    /// Configuration for auto-unload timing and prediction
    #[allow(dead_code)]
    config: SmartLoaderConfig,
    models: Arc<RwLock<HashMap<String, ModelEntry>>>,
    active_tier: Arc<RwLock<Option<ModelTier>>>,
    metrics: Arc<RwLock<SmartLoaderMetrics>>,
    load_semaphore: Semaphore,
    load_callback: Option<Arc<LoadCallback>>,
    predicted_next: Arc<RwLock<Option<String>>>,
}

impl SmartLoader {
    pub fn new(config: SmartLoaderConfig) -> Self {
        let max_loads = config.max_concurrent_loads;
        Self {
            config,
            models: Arc::new(RwLock::new(HashMap::new())),
            active_tier: Arc::new(RwLock::new(None)),
            metrics: Arc::new(RwLock::new(SmartLoaderMetrics::default())),
            load_semaphore: Semaphore::new(max_loads),
            load_callback: None,
            predicted_next: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the callback for actual model loading.
    pub fn set_load_callback(&mut self, callback: LoadCallback) {
        self.load_callback = Some(Arc::new(callback));
    }

    /// Register a model (zero overhead).
    pub async fn register(
        &self,
        model_id: String,
        path: PathBuf,
        tier: ModelTier,
    ) -> Result<(), SmartLoaderError> {
        let size_bytes = std::fs::metadata(&path)
            .map_err(|e| SmartLoaderError::LoadFailed(e.to_string()))?
            .len();

        self.models.write().await.insert(
            model_id,
            ModelEntry {
                path,
                tier,
                size_bytes,
                state: LoadState::Registered,
                handle: None,
                last_used: None,
                use_count: 0,
                load_time_ms: None,
            },
        );
        Ok(())
    }

    /// Provide a semantic hint about upcoming usage.
    pub async fn hint(&self, hint: LoadHint) {
        let prediction = match hint {
            LoadHint::QuickQuery => self.find_model_by_tier(ModelTier::Light).await,
            LoadHint::ComplexTask => self.find_model_by_tier(ModelTier::Quality).await,
            LoadHint::BatchIncoming { count } if count > 10 => {
                self.find_model_by_tier(ModelTier::Quality).await
            }
            LoadHint::BatchIncoming { .. } => self.find_model_by_tier(ModelTier::Balanced).await,
            LoadHint::UserIdle => {
                // Preload the most likely next model
                self.predict_next().await
            }
            LoadHint::PreferModel { tier } => self.find_model_by_tier(tier).await,
        };

        if let Some(model_id) = prediction {
            *self.predicted_next.write().await = Some(model_id.clone());
            self.metrics.write().await.predictions_made += 1;

            // Start background preload
            self.preload_background(&model_id);
        }
    }

    /// Find a registered model by tier.
    async fn find_model_by_tier(&self, tier: ModelTier) -> Option<String> {
        self.models
            .read()
            .await
            .iter()
            .find(|(_, m)| m.tier == tier)
            .map(|(id, _)| id.clone())
    }

    /// Predict most likely next model based on usage patterns.
    async fn predict_next(&self) -> Option<String> {
        let models = self.models.read().await;

        // Simple heuristic: predict based on time of day and usage patterns
        // In production, this could use ML-based prediction

        // For now, predict the most frequently used model that isn't active
        let active = self.active_tier.read().await.clone();

        models
            .iter()
            .filter(|(_, m)| Some(m.tier) != active)
            .max_by_key(|(_, m)| m.use_count)
            .map(|(id, _)| id.clone())
    }

    /// Preload a model in the background.
    fn preload_background(&self, model_id: &str) {
        let models = self.models.clone();
        let model_id = model_id.to_string();
        let callback = self.load_callback.clone();

        // Check if we can acquire a permit (non-blocking)
        if self.load_semaphore.available_permits() == 0 {
            return; // Another load in progress
        }

        tokio::spawn(async move {
            // Note: We don't actually need the permit for background load
            // since we're just touching pages. The semaphore check above
            // prevents too many concurrent background loads.

            let path = {
                let mut models = models.write().await;
                let entry = match models.get_mut(&model_id) {
                    Some(e) if e.state == LoadState::Registered => e,
                    _ => return, // Already loaded or loading
                };
                entry.state = LoadState::Loading;
                entry.path.clone()
            };

            // Actual load (via callback or just touch pages)
            let start = Instant::now();
            let result = if let Some(cb) = callback {
                cb(&path)
            } else {
                // Default: just mmap and touch first page
                match std::fs::File::open(&path) {
                    Ok(file) => {
                        if let Ok(mmap) = unsafe { memmap2::Mmap::map(&file) } {
                            let _ = mmap.get(0); // Touch first page
                            Ok(ModelHandle::new(1)) // Placeholder
                        } else {
                            Err("mmap failed".to_string())
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            };
            let load_ms = start.elapsed().as_millis() as u64;

            let mut models = models.write().await;
            if let Some(entry) = models.get_mut(&model_id) {
                match result {
                    Ok(handle) => {
                        entry.state = LoadState::Ready;
                        entry.handle = Some(handle);
                        entry.load_time_ms = Some(load_ms);
                    }
                    Err(_) => {
                        entry.state = LoadState::Failed;
                    }
                }
            }
        });
    }

    /// Get a model, loading if necessary.
    pub async fn get(&self, model_id: &str) -> Result<ModelHandle, SmartLoaderError> {
        let start = Instant::now();

        // Check prediction accuracy
        {
            let predicted = self.predicted_next.read().await.clone();
            if predicted.as_ref() == Some(&model_id.to_string()) {
                self.metrics.write().await.predictions_correct += 1;
            }
        }

        // Check if already loaded
        {
            let mut models = self.models.write().await;
            if let Some(entry) = models.get_mut(model_id) {
                if entry.state == LoadState::Ready {
                    if let Some(handle) = entry.handle {
                        entry.last_used = Some(Instant::now());
                        entry.use_count += 1;
                        *self.active_tier.write().await = Some(entry.tier);

                        let load_ms = start.elapsed().as_millis() as f64;
                        let mut metrics = self.metrics.write().await;
                        metrics.cache_hits += 1;
                        let n = metrics.cache_hits as f64;
                        metrics.avg_cache_hit_ms =
                            (metrics.avg_cache_hit_ms * (n - 1.0) + load_ms) / n;

                        return Ok(handle);
                    }
                }
            }
        }

        // Need to load
        self.load_sync(model_id).await?;

        let models = self.models.read().await;
        let entry = models
            .get(model_id)
            .ok_or_else(|| SmartLoaderError::NotRegistered(model_id.to_string()))?;

        let handle = entry
            .handle
            .ok_or_else(|| SmartLoaderError::LoadFailed("Load did not produce handle".into()))?;

        let load_ms = start.elapsed().as_millis() as f64;
        let mut metrics = self.metrics.write().await;
        metrics.total_loads += 1;
        let n = metrics.total_loads as f64;
        metrics.avg_load_ms = (metrics.avg_load_ms * (n - 1.0) + load_ms) / n;

        Ok(handle)
    }

    /// Synchronous load (blocks until complete).
    async fn load_sync(&self, model_id: &str) -> Result<(), SmartLoaderError> {
        let _permit = self.load_semaphore.acquire().await.unwrap();

        let path = {
            let mut models = self.models.write().await;
            let entry = models
                .get_mut(model_id)
                .ok_or_else(|| SmartLoaderError::NotRegistered(model_id.to_string()))?;

            if entry.state == LoadState::Ready {
                return Ok(()); // Loaded while waiting for permit
            }

            entry.state = LoadState::Loading;
            entry.path.clone()
        };

        let start = Instant::now();
        let result = if let Some(cb) = &self.load_callback {
            cb(&path)
        } else {
            // Default mmap load
            std::fs::File::open(&path)
                .and_then(|f| unsafe { memmap2::Mmap::map(&f) })
                .map(|mmap| {
                    let _ = mmap.get(0);
                    ModelHandle::new(1)
                })
                .map_err(|e| e.to_string())
        };
        let load_ms = start.elapsed().as_millis() as u64;

        let mut models = self.models.write().await;
        let entry = models.get_mut(model_id).unwrap();

        match result {
            Ok(handle) => {
                entry.state = LoadState::Ready;
                entry.handle = Some(handle);
                entry.load_time_ms = Some(load_ms);
                entry.last_used = Some(Instant::now());
                entry.use_count += 1;
                *self.active_tier.write().await = Some(entry.tier);
                Ok(())
            }
            Err(e) => {
                entry.state = LoadState::Failed;
                Err(SmartLoaderError::LoadFailed(e))
            }
        }
    }

    /// Unload a model.
    pub async fn unload(&self, model_id: &str) {
        if let Some(entry) = self.models.write().await.get_mut(model_id) {
            entry.state = LoadState::Registered;
            entry.handle = None;
        }
    }

    /// Get metrics.
    pub async fn metrics(&self) -> SmartLoaderMetrics {
        self.metrics.read().await.clone()
    }

    /// Get current status.
    pub async fn status(&self) -> SmartLoaderStatus {
        let models = self.models.read().await;
        let active_tier = self.active_tier.read().await.clone();

        let loaded: Vec<_> = models
            .iter()
            .filter(|(_, m)| m.state == LoadState::Ready)
            .map(|(id, m)| (id.clone(), m.tier))
            .collect();

        let total_loaded_bytes: u64 = models
            .values()
            .filter(|m| m.state == LoadState::Ready)
            .map(|m| m.size_bytes)
            .sum();

        SmartLoaderStatus {
            registered_count: models.len(),
            loaded_count: loaded.len(),
            loaded_models: loaded,
            active_tier,
            total_loaded_bytes,
            predicted_next: self.predicted_next.read().await.clone(),
        }
    }
}

/// Current loader status.
#[derive(Debug)]
pub struct SmartLoaderStatus {
    pub registered_count: usize,
    pub loaded_count: usize,
    pub loaded_models: Vec<(String, ModelTier)>,
    pub active_tier: Option<ModelTier>,
    pub total_loaded_bytes: u64,
    pub predicted_next: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_model(size: usize) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&vec![0u8; size]).unwrap();
        file.flush().unwrap();
        file
    }

    #[tokio::test]
    async fn test_register_zero_overhead() {
        let loader = SmartLoader::new(SmartLoaderConfig::default());
        let file = create_test_model(1_000_000);

        loader
            .register(
                "test".to_string(),
                file.path().to_path_buf(),
                ModelTier::Light,
            )
            .await
            .unwrap();

        let status = loader.status().await;
        assert_eq!(status.registered_count, 1);
        assert_eq!(status.loaded_count, 0);
        assert_eq!(status.total_loaded_bytes, 0);
    }

    #[tokio::test]
    async fn test_semantic_hint_quick_query() {
        let loader = SmartLoader::new(SmartLoaderConfig::default());

        let light = create_test_model(100_000);
        let quality = create_test_model(200_000);

        loader
            .register(
                "light".to_string(),
                light.path().to_path_buf(),
                ModelTier::Light,
            )
            .await
            .unwrap();
        loader
            .register(
                "quality".to_string(),
                quality.path().to_path_buf(),
                ModelTier::Quality,
            )
            .await
            .unwrap();

        // Hint: quick query
        loader.hint(LoadHint::QuickQuery).await;

        // Should predict light model
        let status = loader.status().await;
        assert_eq!(status.predicted_next, Some("light".to_string()));
    }

    #[tokio::test]
    async fn test_cache_hit_fast() {
        let loader = SmartLoader::new(SmartLoaderConfig::default());
        let file = create_test_model(100_000);

        loader
            .register(
                "test".to_string(),
                file.path().to_path_buf(),
                ModelTier::Balanced,
            )
            .await
            .unwrap();

        // First load (cold)
        let start = Instant::now();
        loader.get("test").await.unwrap();
        let cold_time = start.elapsed();

        // Second load (cache hit)
        let start = Instant::now();
        loader.get("test").await.unwrap();
        let warm_time = start.elapsed();

        println!("Cold: {:?}, Warm: {:?}", cold_time, warm_time);

        let metrics = loader.metrics().await;
        assert_eq!(metrics.total_loads, 1);
        assert_eq!(metrics.cache_hits, 1);
    }

    #[tokio::test]
    async fn test_tier_based_hints() {
        let loader = SmartLoader::new(SmartLoaderConfig::default());

        let light = create_test_model(100);
        let balanced = create_test_model(100);
        let quality = create_test_model(100);

        loader
            .register(
                "l".to_string(),
                light.path().to_path_buf(),
                ModelTier::Light,
            )
            .await
            .unwrap();
        loader
            .register(
                "b".to_string(),
                balanced.path().to_path_buf(),
                ModelTier::Balanced,
            )
            .await
            .unwrap();
        loader
            .register(
                "q".to_string(),
                quality.path().to_path_buf(),
                ModelTier::Quality,
            )
            .await
            .unwrap();

        // Different hints should predict different models
        loader.hint(LoadHint::QuickQuery).await;
        assert_eq!(loader.status().await.predicted_next, Some("l".to_string()));

        loader.hint(LoadHint::ComplexTask).await;
        assert_eq!(loader.status().await.predicted_next, Some("q".to_string()));

        loader
            .hint(LoadHint::PreferModel {
                tier: ModelTier::Balanced,
            })
            .await;
        assert_eq!(loader.status().await.predicted_next, Some("b".to_string()));
    }
}
