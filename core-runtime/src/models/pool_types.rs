//! Types and configuration for the model pool.
//!
//! Extracted from `pool.rs` for Section 4 compliance.

use std::time::Duration;
use thiserror::Error;

use super::registry::ModelHandle;

#[derive(Error, Debug)]
pub enum PoolError {
    #[error("Pool capacity exceeded: {current}/{max}")]
    CapacityExceeded { current: usize, max: usize },

    #[error("Model not in pool: {0}")]
    ModelNotFound(String),

    #[error("Model already in pool: {0}")]
    AlreadyLoaded(String),

    #[error("Eviction failed: no evictable models")]
    EvictionFailed,
}

/// Model tier for prioritized eviction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelTier {
    /// CI/Testing - lowest priority, first to evict
    Testing = 0,
    /// Default installation tier
    Default = 1,
    /// Quality/Production tier - highest priority
    Quality = 2,
}

/// Configuration for the model pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of models to keep in pool
    pub max_models: usize,
    /// Maximum total memory for pooled models (bytes)
    pub max_memory_bytes: usize,
    /// Warmup prompt for new models
    pub warmup_prompt: String,
    /// Enable background preloading
    pub enable_preload: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_models: 3,
            max_memory_bytes: 8 * 1024 * 1024 * 1024, // 8 GB
            warmup_prompt: "Hello".to_string(),
            enable_preload: true,
        }
    }
}

/// Metrics for pool operations.
#[derive(Debug, Default, Clone)]
pub struct PoolMetrics {
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub evictions: u64,
    pub warmups_completed: u64,
    pub avg_switch_latency_ns: u64,
}

/// Result of switching to a pooled model.
#[derive(Debug)]
pub struct SwitchResult {
    pub handle: ModelHandle,
    pub switch_latency: Duration,
    pub was_preloaded: bool,
    pub was_warmed: bool,
}

/// Current pool status.
#[derive(Debug)]
pub struct PoolStatus {
    pub model_count: usize,
    pub total_memory_bytes: usize,
    pub active_model: Option<String>,
    pub loaded_models: Vec<String>,
    pub metrics: PoolMetrics,
}
