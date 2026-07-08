//! Hot-swap logic for zero-downtime model replacement.
//!
//! Orchestrates preload, drain, and route swap for seamless transitions.

use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;

use super::drain::{DrainError, FlightTracker};
use super::manifest::ModelManifest;
use super::preload::{ModelPreloader, PreloadError};
use super::registry::{ModelHandle, ModelRegistry};
use super::router::ModelRouter;

#[derive(Error, Debug)]
pub enum SwapError {
    #[error("Route not found: {0}")]
    RouteNotFound(String),

    #[error("Swap already in progress")]
    SwapInProgress,

    #[error("Preload failed: {0}")]
    PreloadFailed(#[from] PreloadError),

    #[error("Drain timeout")]
    DrainTimeout,
}

/// Result of a successful swap operation.
#[derive(Debug)]
pub struct SwapResult {
    pub old_handle: ModelHandle,
    pub new_handle: ModelHandle,
    pub drain_duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwapState {
    Idle,
    Preparing,
    Draining,
    Swapping,
}

/// Manages zero-downtime model hot-swapping.
pub struct SwapManager {
    state: Arc<RwLock<SwapState>>,
    registry: Arc<ModelRegistry>,
    router: Arc<ModelRouter>,
    flight_tracker: Arc<FlightTracker>,
    preloader: ModelPreloader,
}

impl SwapManager {
    pub fn new(
        registry: Arc<ModelRegistry>,
        router: Arc<ModelRouter>,
        flight_tracker: Arc<FlightTracker>,
    ) -> Self {
        let preloader = ModelPreloader::new(registry.clone());
        Self {
            state: Arc::new(RwLock::new(SwapState::Idle)),
            registry,
            router,
            flight_tracker,
            preloader,
        }
    }

    /// Execute zero-downtime swap:
    /// 1. Preload new model
    /// 2. Drain in-flight requests for old model
    /// 3. Atomically swap route
    /// 4. Unregister old model
    pub async fn execute_swap(
        &self,
        model_id: &str,
        new_manifest: ModelManifest,
        drain_timeout: Duration,
    ) -> Result<SwapResult, SwapError> {
        // Acquire swap lock
        {
            let mut state = self.state.write().await;
            if *state != SwapState::Idle {
                return Err(SwapError::SwapInProgress);
            }
            *state = SwapState::Preparing;
        }

        // Get old handle
        let old_handle = match self.router.resolve(model_id).await {
            Some(h) => h,
            None => {
                self.reset_state().await;
                return Err(SwapError::RouteNotFound(model_id.to_string()));
            }
        };

        // Preload new model
        let preloaded = match self.preloader.preload(new_manifest).await {
            Ok(p) => p,
            Err(e) => {
                self.reset_state().await;
                return Err(SwapError::PreloadFailed(e));
            }
        };
        let new_handle = preloaded.handle;

        // Drain in-flight requests
        {
            *self.state.write().await = SwapState::Draining;
        }
        let drain_start = std::time::Instant::now();
        if let Err(DrainError::Timeout) = self.flight_tracker.drain(old_handle, drain_timeout).await
        {
            // Abort: unregister preloaded model, keep old route
            self.preloader.abort(preloaded).await;
            self.reset_state().await;
            return Err(SwapError::DrainTimeout);
        }
        let drain_duration = drain_start.elapsed();

        // Atomic swap
        {
            *self.state.write().await = SwapState::Swapping;
        }
        self.router.swap_route(model_id, new_handle).await;

        // Cleanup old model
        self.registry.unregister(old_handle).await;
        self.flight_tracker.remove(old_handle).await;

        self.reset_state().await;

        Ok(SwapResult {
            old_handle,
            new_handle,
            drain_duration,
        })
    }

    async fn reset_state(&self) {
        *self.state.write().await = SwapState::Idle;
    }

    pub async fn is_idle(&self) -> bool {
        *self.state.read().await == SwapState::Idle
    }
}
