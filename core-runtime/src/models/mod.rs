//! Model management module for CORE Runtime.
//!
//! Handles model loading, registry tracking, manifest parsing, and hot-swap operations.

pub mod backend_capabilities;
pub mod backend_dispatch;
pub mod backend_selection;
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

mod containment;
mod drain;
pub mod integrity;
pub mod lifecycle;
mod loader;
pub mod model_ops;
mod preload;
pub mod registry;
mod router;
mod swap;

// v0.5.0: Model registry enhancements
pub mod history;
pub mod persistence;
pub mod search;
pub mod version;

pub use backend_dispatch::{load_model_dispatch, load_model_dispatch_for_workers, BackendChoice};
pub use drain::{DrainError, FlightGuard, FlightTracker};
pub use history::{VersionHistory, VersionHistoryEntry, VersionSource};
pub use lifecycle::{LifecycleError, ModelLifecycle, MAX_RELEASE_WAIT};
pub use loader::{LoadError, MappedModel, ModelLoader, ModelMetadata, ModelPath};
pub use manifest::{ModelArchitecture, ModelCapability, ModelManifest};
pub use model_ops::{
    load_model_from_path, load_model_from_path_for_workers, prepare_model, prepare_model_admitted,
    prepare_model_admitted_for_workers, register_prepared, unload_model, LoadedModel, ModelOpError,
    PreparedModel,
};
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
