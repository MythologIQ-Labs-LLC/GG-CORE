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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_report_healthy() {
        let checker = HealthChecker::new(HealthConfig {
            require_model_loaded: true,
            max_queue_depth: 10,
        });

        let report = checker.report(ShutdownState::Running, 1, 1024, 5);

        assert_eq!(report.state, HealthState::Healthy);
        assert!(report.ready);
        assert!(report.accepting_requests);
        assert_eq!(report.models_loaded, 1);
        assert_eq!(report.memory_used_bytes, 1024);
        assert_eq!(report.queue_depth, 5);
    }

    #[test]
    fn test_health_checker_report_degraded_no_model() {
        let checker = HealthChecker::new(HealthConfig {
            require_model_loaded: true,
            max_queue_depth: 10,
        });

        let report = checker.report(ShutdownState::Running, 0, 1024, 5);

        assert_eq!(report.state, HealthState::Degraded);
        assert!(!report.ready);
        assert!(report.accepting_requests);
        assert_eq!(report.models_loaded, 0);
        assert_eq!(report.memory_used_bytes, 1024);
        assert_eq!(report.queue_depth, 5);
    }

    #[test]
    fn test_health_checker_report_healthy_no_model_required() {
        let checker = HealthChecker::new(HealthConfig {
            require_model_loaded: false,
            max_queue_depth: 10,
        });

        let report = checker.report(ShutdownState::Running, 0, 1024, 5);

        assert_eq!(report.state, HealthState::Healthy);
        assert!(report.ready);
        assert!(report.accepting_requests);
        assert_eq!(report.models_loaded, 0);
        assert_eq!(report.memory_used_bytes, 1024);
        assert_eq!(report.queue_depth, 5);
    }

    #[test]
    fn test_health_checker_report_degraded_queue_full() {
        let checker = HealthChecker::new(HealthConfig {
            require_model_loaded: false,
            max_queue_depth: 10,
        });

        // Queue is at max capacity
        let report = checker.report(ShutdownState::Running, 1, 1024, 10);

        assert_eq!(report.state, HealthState::Degraded);
        assert!(!report.ready);
        assert!(report.accepting_requests);
        assert_eq!(report.models_loaded, 1);
        assert_eq!(report.memory_used_bytes, 1024);
        assert_eq!(report.queue_depth, 10);
    }

    #[test]
    fn test_health_checker_report_unhealthy_draining() {
        let checker = HealthChecker::new(HealthConfig {
            require_model_loaded: false,
            max_queue_depth: 10,
        });

        let report = checker.report(ShutdownState::Draining, 1, 1024, 5);

        assert_eq!(report.state, HealthState::Unhealthy);
        assert!(!report.ready);
        assert!(!report.accepting_requests);
        assert_eq!(report.models_loaded, 1);
        assert_eq!(report.memory_used_bytes, 1024);
        assert_eq!(report.queue_depth, 5);
    }

    #[test]
    fn test_health_checker_report_unhealthy_stopped() {
        let checker = HealthChecker::new(HealthConfig {
            require_model_loaded: false,
            max_queue_depth: 10,
        });

        let report = checker.report(ShutdownState::Stopped, 1, 1024, 5);

        assert_eq!(report.state, HealthState::Unhealthy);
        assert!(!report.ready);
        assert!(!report.accepting_requests);
        assert_eq!(report.models_loaded, 1);
        assert_eq!(report.memory_used_bytes, 1024);
        assert_eq!(report.queue_depth, 5);
    }

    #[test]
    fn test_is_ready_running_state() {
        let checker = HealthChecker::default();
        // Default requires 0 models loaded and queue depth < 1000
        assert!(checker.is_ready(ShutdownState::Running, 0, 0));
        assert!(checker.is_ready(ShutdownState::Running, 1, 999));
    }

    #[test]
    fn test_is_ready_shutdown_states() {
        let checker = HealthChecker::default();
        assert!(!checker.is_ready(ShutdownState::Draining, 1, 0));
        assert!(!checker.is_ready(ShutdownState::Stopped, 1, 0));
    }

    #[test]
    fn test_is_ready_require_model_loaded() {
        let config = HealthConfig {
            require_model_loaded: true,
            max_queue_depth: 1000,
        };
        let checker = HealthChecker::new(config);

        // Not ready if no models are loaded
        assert!(!checker.is_ready(ShutdownState::Running, 0, 0));
        // Ready if models are loaded
        assert!(checker.is_ready(ShutdownState::Running, 1, 0));
        assert!(checker.is_ready(ShutdownState::Running, 5, 0));
    }

    #[test]
    fn test_is_ready_max_queue_depth() {
        let config = HealthConfig {
            require_model_loaded: false,
            max_queue_depth: 10,
        };
        let checker = HealthChecker::new(config);

        assert!(checker.is_ready(ShutdownState::Running, 0, 0));
        assert!(checker.is_ready(ShutdownState::Running, 0, 9));

        // Not ready if queue is at or above max depth
        assert!(!checker.is_ready(ShutdownState::Running, 0, 10));
        assert!(!checker.is_ready(ShutdownState::Running, 0, 15));
    }
}
