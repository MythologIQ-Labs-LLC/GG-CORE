//! Tier Synergy: Smart integration of tiered models with speculative decoding.
//!
//! When both Light and Quality tiers are available, automatically enables
//! speculative decoding for 1.5-2x throughput improvement.
//!
//! **Innovation**: Uses the memory-efficient SmartLoader's lazy loading combined
//! with speculative decoding's draft-verify paradigm:
//! - Light tier (Qwen 0.5B) serves as the draft model
//! - Quality tier (Phi-3 Mini) serves as the verification model
//! - Balanced tier (Qwen 1.5B) can serve as either depending on load

use std::sync::Arc;
use tokio::sync::RwLock;

use super::smart_loader::{LoadHint, ModelTier, SmartLoader, SmartLoaderError};
use crate::engine::speculative_v2::{SpeculativeConfig, SpeculativeStats};
use crate::models::registry::ModelHandle;

/// Synergy mode for tiered model usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynergyMode {
    /// Use single model (no speculation)
    Single,
    /// Speculative decoding with Light as draft, Quality as target
    SpeculativeLightQuality,
    /// Speculative decoding with Light as draft, Balanced as target
    SpeculativeLightBalanced,
    /// Speculative decoding with Balanced as draft, Quality as target
    SpeculativeBalancedQuality,
}

/// Result of a synergy-aware model request.
#[derive(Debug)]
pub struct SynergyResult {
    /// Primary model handle
    pub primary_handle: ModelHandle,
    /// Draft model handle (if speculative mode)
    pub draft_handle: Option<ModelHandle>,
    /// Active synergy mode
    pub mode: SynergyMode,
    /// Whether draft model is already loaded (zero additional latency)
    pub draft_ready: bool,
}

/// Tier synergy manager that combines smart loading with speculative decoding.
pub struct TierSynergy {
    loader: Arc<SmartLoader>,
    mode: Arc<RwLock<SynergyMode>>,
    spec_config: SpeculativeConfig,
    stats: Arc<RwLock<SpeculativeStats>>,
    /// Model IDs by tier
    tier_models: Arc<RwLock<TierModels>>,
}

/// Mapping of tiers to model IDs.
#[derive(Debug, Default)]
struct TierModels {
    light: Option<String>,
    balanced: Option<String>,
    quality: Option<String>,
}

impl TierSynergy {
    pub fn new(loader: Arc<SmartLoader>) -> Self {
        Self {
            loader,
            mode: Arc::new(RwLock::new(SynergyMode::Single)),
            spec_config: SpeculativeConfig::default(),
            stats: Arc::new(RwLock::new(SpeculativeStats::default())),
            tier_models: Arc::new(RwLock::new(TierModels::default())),
        }
    }

    /// Set custom speculative config.
    pub fn with_spec_config(mut self, config: SpeculativeConfig) -> Self {
        self.spec_config = config;
        self
    }

    /// Register a model with its tier for synergy tracking.
    pub async fn register_tier(&self, model_id: &str, tier: ModelTier) {
        let mut tiers = self.tier_models.write().await;
        match tier {
            ModelTier::Light => tiers.light = Some(model_id.to_string()),
            ModelTier::Balanced => tiers.balanced = Some(model_id.to_string()),
            ModelTier::Quality => tiers.quality = Some(model_id.to_string()),
        }
        drop(tiers);

        // Auto-detect best synergy mode
        self.detect_optimal_mode().await;
    }

    /// Detect and set optimal synergy mode based on available tiers.
    async fn detect_optimal_mode(&self) {
        let tiers = self.tier_models.read().await;

        let new_mode = if tiers.light.is_some() && tiers.quality.is_some() {
            // Best: Light drafts, Quality verifies
            SynergyMode::SpeculativeLightQuality
        } else if tiers.light.is_some() && tiers.balanced.is_some() {
            // Good: Light drafts, Balanced verifies
            SynergyMode::SpeculativeLightBalanced
        } else if tiers.balanced.is_some() && tiers.quality.is_some() {
            // Acceptable: Balanced drafts, Quality verifies
            SynergyMode::SpeculativeBalancedQuality
        } else {
            // Fallback: Single model mode
            SynergyMode::Single
        };

        *self.mode.write().await = new_mode;
    }

    /// Get current synergy mode.
    pub async fn mode(&self) -> SynergyMode {
        *self.mode.read().await
    }

    /// Request a model for a task, with automatic synergy selection.
    pub async fn request(&self, hint: LoadHint) -> Result<SynergyResult, SmartLoaderError> {
        // First, inform loader of the hint
        self.loader.hint(hint).await;

        let mode = self.mode().await;
        let tiers = self.tier_models.read().await;

        match (mode, hint) {
            // Speculative modes prefer drafting + verification
            (SynergyMode::SpeculativeLightQuality, LoadHint::ComplexTask) => {
                let quality_id = tiers.quality.as_ref().unwrap();
                let light_id = tiers.light.as_ref().unwrap();

                // Load quality first (target), hint light (draft)
                let primary = self.loader.get(quality_id).await?;
                self.loader.hint(LoadHint::PreferModel { tier: ModelTier::Light }).await;

                // Check if light is already loaded
                let status = self.loader.status().await;
                let draft_ready = status
                    .loaded_models
                    .iter()
                    .any(|(id, _)| id == light_id);

                let draft_handle = if draft_ready {
                    Some(self.loader.get(light_id).await?)
                } else {
                    None
                };

                Ok(SynergyResult {
                    primary_handle: primary,
                    draft_handle,
                    mode,
                    draft_ready,
                })
            }

            // Quick queries use light tier only
            (_, LoadHint::QuickQuery) => {
                let model_id = tiers
                    .light
                    .as_ref()
                    .or(tiers.balanced.as_ref())
                    .ok_or_else(|| SmartLoaderError::NotRegistered("no light tier".into()))?;

                let handle = self.loader.get(model_id).await?;
                Ok(SynergyResult {
                    primary_handle: handle,
                    draft_handle: None,
                    mode: SynergyMode::Single,
                    draft_ready: false,
                })
            }

            // Batch processing uses speculative if available
            (SynergyMode::SpeculativeLightQuality, LoadHint::BatchIncoming { count })
                if count > 5 =>
            {
                let quality_id = tiers.quality.as_ref().unwrap();
                let light_id = tiers.light.as_ref().unwrap();

                // Preload both for batch
                let primary = self.loader.get(quality_id).await?;
                let draft = self.loader.get(light_id).await?;

                Ok(SynergyResult {
                    primary_handle: primary,
                    draft_handle: Some(draft),
                    mode,
                    draft_ready: true,
                })
            }

            // Default: use appropriate single model
            _ => {
                let model_id = self.select_model_for_hint(hint, &tiers).await?;
                let handle = self.loader.get(&model_id).await?;

                Ok(SynergyResult {
                    primary_handle: handle,
                    draft_handle: None,
                    mode: SynergyMode::Single,
                    draft_ready: false,
                })
            }
        }
    }

    /// Select best single model for a hint.
    async fn select_model_for_hint(
        &self,
        hint: LoadHint,
        tiers: &TierModels,
    ) -> Result<String, SmartLoaderError> {
        let tier = match hint {
            LoadHint::QuickQuery => ModelTier::Light,
            LoadHint::ComplexTask => ModelTier::Quality,
            LoadHint::BatchIncoming { count } if count > 10 => ModelTier::Quality,
            LoadHint::BatchIncoming { .. } => ModelTier::Balanced,
            LoadHint::UserIdle => ModelTier::Balanced,
            LoadHint::PreferModel { tier } => tier,
        };

        // Find model for tier, falling back to alternatives
        match tier {
            ModelTier::Light => tiers
                .light
                .clone()
                .or_else(|| tiers.balanced.clone())
                .or_else(|| tiers.quality.clone()),
            ModelTier::Balanced => tiers
                .balanced
                .clone()
                .or_else(|| tiers.quality.clone())
                .or_else(|| tiers.light.clone()),
            ModelTier::Quality => tiers
                .quality
                .clone()
                .or_else(|| tiers.balanced.clone())
                .or_else(|| tiers.light.clone()),
        }
        .ok_or_else(|| SmartLoaderError::NotRegistered("no models registered".into()))
    }

    /// Get speculative stats.
    pub async fn stats(&self) -> SpeculativeStats {
        self.stats.read().await.clone()
    }

    /// Get synergy status.
    pub async fn status(&self) -> SynergyStatus {
        let mode = self.mode().await;
        let tiers = self.tier_models.read().await;
        let loader_status = self.loader.status().await;

        SynergyStatus {
            mode,
            available_tiers: vec![
                tiers.light.is_some(),
                tiers.balanced.is_some(),
                tiers.quality.is_some(),
            ],
            loaded_tiers: loader_status
                .loaded_models
                .iter()
                .map(|(_, tier)| *tier)
                .collect(),
            spec_config: self.spec_config.clone(),
        }
    }
}

/// Current synergy status.
#[derive(Debug)]
pub struct SynergyStatus {
    pub mode: SynergyMode,
    /// [light, balanced, quality] availability
    pub available_tiers: Vec<bool>,
    pub loaded_tiers: Vec<ModelTier>,
    pub spec_config: SpeculativeConfig,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::registry::ModelHandle;
    use crate::models::smart_loader::SmartLoaderConfig;
    use crate::models::smart_loader_types::LoadCallback;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tempfile::NamedTempFile;

    fn test_callback() -> LoadCallback {
        let counter = std::sync::Arc::new(AtomicU64::new(100));
        Box::new(move |_path| {
            let id = counter.fetch_add(1, Ordering::SeqCst);
            Ok(ModelHandle::new(id))
        })
    }

    fn create_test_model(size: usize) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&vec![0u8; size]).unwrap();
        file.flush().unwrap();
        file
    }

    #[tokio::test]
    async fn test_synergy_auto_detect_mode() {
        let loader = Arc::new(SmartLoader::new(SmartLoaderConfig::default(), test_callback()));
        let synergy = TierSynergy::new(loader.clone());

        let light = create_test_model(100);
        let quality = create_test_model(200);

        // Register tiers
        loader
            .register("light".into(), light.path().to_path_buf(), ModelTier::Light)
            .await
            .unwrap();
        loader
            .register("quality".into(), quality.path().to_path_buf(), ModelTier::Quality)
            .await
            .unwrap();

        synergy.register_tier("light", ModelTier::Light).await;
        synergy.register_tier("quality", ModelTier::Quality).await;

        // Should auto-detect speculative mode
        assert_eq!(
            synergy.mode().await,
            SynergyMode::SpeculativeLightQuality
        );
    }

    #[tokio::test]
    async fn test_synergy_request_quick_query() {
        let loader = Arc::new(SmartLoader::new(SmartLoaderConfig::default(), test_callback()));
        let synergy = TierSynergy::new(loader.clone());

        let light = create_test_model(100);
        loader
            .register("light".into(), light.path().to_path_buf(), ModelTier::Light)
            .await
            .unwrap();
        synergy.register_tier("light", ModelTier::Light).await;

        let result = synergy.request(LoadHint::QuickQuery).await.unwrap();
        assert_eq!(result.mode, SynergyMode::Single);
        assert!(result.draft_handle.is_none());
    }

    #[tokio::test]
    async fn test_synergy_complex_task_speculative() {
        let loader = Arc::new(SmartLoader::new(SmartLoaderConfig::default(), test_callback()));
        let synergy = TierSynergy::new(loader.clone());

        let light = create_test_model(100);
        let quality = create_test_model(200);

        loader
            .register("light".into(), light.path().to_path_buf(), ModelTier::Light)
            .await
            .unwrap();
        loader
            .register("quality".into(), quality.path().to_path_buf(), ModelTier::Quality)
            .await
            .unwrap();

        synergy.register_tier("light", ModelTier::Light).await;
        synergy.register_tier("quality", ModelTier::Quality).await;

        let result = synergy.request(LoadHint::ComplexTask).await.unwrap();
        assert_eq!(result.mode, SynergyMode::SpeculativeLightQuality);
    }

    #[tokio::test]
    async fn test_synergy_fallback_single_tier() {
        let loader = Arc::new(SmartLoader::new(SmartLoaderConfig::default(), test_callback()));
        let synergy = TierSynergy::new(loader.clone());

        let balanced = create_test_model(150);
        loader
            .register("balanced".into(), balanced.path().to_path_buf(), ModelTier::Balanced)
            .await
            .unwrap();
        synergy.register_tier("balanced", ModelTier::Balanced).await;

        // Should fall back to single mode
        assert_eq!(synergy.mode().await, SynergyMode::Single);

        let result = synergy.request(LoadHint::ComplexTask).await.unwrap();
        assert_eq!(result.mode, SynergyMode::Single);
    }
}
