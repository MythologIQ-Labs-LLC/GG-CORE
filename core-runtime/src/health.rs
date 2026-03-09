//! Health check support for CORE Runtime.
//!
//! Provides liveness, readiness, and full health report capabilities
//! for orchestrator integration (Kubernetes, systemd).

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::shutdown::ShutdownState;

/// Overall health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthState {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Detailed health report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub state: HealthState,
    pub ready: bool,
    pub accepting_requests: bool,
    pub models_loaded: usize,
    pub memory_used_bytes: usize,
    pub queue_depth: usize,
    pub uptime_secs: u64,
}

/// Health check configuration.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    pub require_model_loaded: bool,
    pub max_queue_depth: usize,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            require_model_loaded: false,
            max_queue_depth: 1000,
        }
    }
}

/// Aggregates health information from runtime components.
pub struct HealthChecker {
    config: HealthConfig,
    start_time: Instant,
}

impl HealthChecker {
    pub fn new(config: HealthConfig) -> Self {
        Self {
            config,
            start_time: Instant::now(),
        }
    }

    /// Check liveness: process is responsive.
    pub fn is_alive(&self) -> bool {
        true
    }

    /// Check readiness: accepting traffic.
    pub fn is_ready(&self, shutdown_state: ShutdownState, models: usize, queue: usize) -> bool {
        if shutdown_state != ShutdownState::Running {
            return false;
        }
        if self.config.require_model_loaded && models == 0 {
            return false;
        }
        if queue >= self.config.max_queue_depth {
            return false;
        }
        true
    }

    /// Generate full health report.
    pub fn report(
        &self,
        shutdown_state: ShutdownState,
        models: usize,
        memory_bytes: usize,
        queue: usize,
    ) -> HealthReport {
        let accepting = shutdown_state == ShutdownState::Running;
        let ready = self.is_ready(shutdown_state, models, queue);
        let state = self.compute_state(shutdown_state, models, queue);

        HealthReport {
            state,
            ready,
            accepting_requests: accepting,
            models_loaded: models,
            memory_used_bytes: memory_bytes,
            queue_depth: queue,
            uptime_secs: self.start_time.elapsed().as_secs(),
        }
    }

    fn compute_state(
        &self,
        shutdown_state: ShutdownState,
        models: usize,
        queue: usize,
    ) -> HealthState {
        if shutdown_state != ShutdownState::Running {
            return HealthState::Unhealthy;
        }
        if self.config.require_model_loaded && models == 0 {
            return HealthState::Degraded;
        }
        if queue >= self.config.max_queue_depth {
            return HealthState::Degraded;
        }
        HealthState::Healthy
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new(HealthConfig::default())
    }
}
