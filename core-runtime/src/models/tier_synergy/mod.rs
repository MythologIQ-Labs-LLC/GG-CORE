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

mod mode;
mod status;
#[cfg(test)]
mod tests;

pub use mode::{SynergyMode, SynergyResult};
pub use status::SynergyStatus;

use std::sync::Arc;
use tokio::sync::RwLock;

use super::smart_loader::{LoadHint, ModelTier, SmartLoader, SmartLoaderError};
use crate::engine::speculative_v2::{SpeculativeConfig, SpeculativeStats};

/// Mapping of tiers to model IDs.
#[derive(Debug, Default)]
struct TierModels {
    light: Option<String>,
    balanced: Option<String>,
    quality: Option<String>,
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
        self.detect_optimal_mode().await;
    }

    /// Detect and set optimal synergy mode based on available tiers.
    async fn detect_optimal_mode(&self) {
        let tiers = self.tier_models.read().await;
        let new_mode = if tiers.light.is_some() && tiers.quality.is_some() {
            SynergyMode::SpeculativeLightQuality
        } else if tiers.light.is_some() && tiers.balanced.is_some() {
            SynergyMode::SpeculativeLightBalanced
        } else if tiers.balanced.is_some() && tiers.quality.is_some() {
            SynergyMode::SpeculativeBalancedQuality
        } else {
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
        self.loader.hint(hint).await;
        let mode = self.mode().await;
        let tiers = self.tier_models.read().await;
        match (mode, hint) {
            (SynergyMode::SpeculativeLightQuality, LoadHint::ComplexTask) => {
                self.request_complex_speculative(&tiers, mode).await
            }
            (_, LoadHint::QuickQuery) => self.request_quick_query_inner(&tiers).await,
            (SynergyMode::SpeculativeLightQuality, LoadHint::BatchIncoming { count })
                if count > 5 =>
            {
                self.request_batch_speculative(&tiers, mode).await
            }
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

    async fn request_complex_speculative(
        &self,
        tiers: &TierModels,
        mode: SynergyMode,
    ) -> Result<SynergyResult, SmartLoaderError> {
        let quality_id = tiers.quality.as_ref().unwrap();
        let light_id = tiers.light.as_ref().unwrap();
        let primary = self.loader.get(quality_id).await?;
        self.loader
            .hint(LoadHint::PreferModel {
                tier: ModelTier::Light,
            })
            .await;
        let status = self.loader.status().await;
        let draft_ready = status.loaded_models.iter().any(|(id, _)| id == light_id);
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

    async fn request_quick_query_inner(
        &self,
        tiers: &TierModels,
    ) -> Result<SynergyResult, SmartLoaderError> {
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

    async fn request_batch_speculative(
        &self,
        tiers: &TierModels,
        mode: SynergyMode,
    ) -> Result<SynergyResult, SmartLoaderError> {
        let quality_id = tiers.quality.as_ref().unwrap();
        let light_id = tiers.light.as_ref().unwrap();
        let primary = self.loader.get(quality_id).await?;
        let draft = self.loader.get(light_id).await?;
        Ok(SynergyResult {
            primary_handle: primary,
            draft_handle: Some(draft),
            mode,
            draft_ready: true,
        })
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
