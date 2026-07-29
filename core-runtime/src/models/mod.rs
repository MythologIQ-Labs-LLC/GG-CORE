//! Model management module for CORE Runtime.
//!
//! Handles model loading, registry tracking, manifest parsing, and hot-swap operations.

pub mod backend_dispatch;
pub mod manifest;
pub mod pool;
pub mod pool_types;
pub mod smart_loader;
mod smart_loader_ops;
pub mod smart_loader_types;
#[cfg(feature = "advanced")]
pub mod speculative_config;
#[cfg(feature = "advanced")]
pub mod tier_synergy;
#[cfg(feature = "advanced")]
pub mod tier_synergy_speculative;

mod drain;
pub mod lifecycle;
mod loader;
mod preload;
pub mod registry;
mod router;
mod swap;

// v0.5.0: Model registry enhancements
pub mod history;
pub mod persistence;
pub mod search;
pub mod version;

pub use backend_dispatch::{load_model_dispatch, BackendChoice};
pub use drain::{DrainError, FlightGuard, FlightTracker};
pub use history::{VersionHistory, VersionHistoryEntry, VersionSource};
pub use lifecycle::{LifecycleError, ModelLifecycle};
pub use loader::{LoadError, MappedModel, ModelLoader, ModelMetadata, ModelPath};
pub use manifest::{ModelArchitecture, ModelCapability, ModelManifest};
pub use persistence::{PersistedModel, PersistenceError, RegistryPersistence, RegistryState};
pub use pool::ModelTier as PoolModelTier;
pub use pool::{ModelPool, PoolConfig, PoolError, PoolMetrics, PoolStatus, SwitchResult};
pub use preload::{ModelPreloader, PreloadError, PreloadedModel};
pub use registry::{LoadedModelInfo, LoadedModelState, ModelHandle, ModelRegistry};
pub use router::{ModelRouter, RouterError};
pub use search::{ModelQuery, ModelQueryBuilder, ModelSearchResult};
pub use smart_loader::ModelTier as SmartModelTier;
pub use smart_loader::{
    LoadHint, SmartLoader, SmartLoaderConfig, SmartLoaderError, SmartLoaderMetrics,
    SmartLoaderStatus,
};
#[cfg(feature = "advanced")]
pub use speculative_config::{AdaptiveMode, AdaptiveSpeculativeConfig};
pub use swap::{SwapError, SwapManager, SwapResult};
#[cfg(feature = "advanced")]
pub use tier_synergy::{SynergyMode, SynergyResult, SynergyStatus, TierSynergy};
#[cfg(feature = "advanced")]
pub use tier_synergy_speculative::{CompatibilityCheck, HardwareProfile, TierSpeculativePlan};
pub use version::{ModelVersion, VersionRange};
