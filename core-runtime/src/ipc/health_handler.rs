//! Health check request handling.
//!
//! Extracted from handler.rs to maintain Section 4 compliance.
//! Uses orchestrator pattern (no auth required).

use std::sync::Arc;

use super::protocol::{HealthCheckResponse, HealthCheckType};
use crate::health::HealthChecker;
use crate::models::ModelRegistry;
use crate::scheduler::RequestQueue;
use crate::shutdown::ShutdownCoordinator;

/// Handles health check requests (orchestrator pattern, no auth).
pub struct HealthHandler {
    health: Arc<HealthChecker>,
    shutdown: Arc<ShutdownCoordinator>,
    model_registry: Arc<ModelRegistry>,
    queue: Arc<RequestQueue>,
}

impl HealthHandler {
    pub fn new(
        health: Arc<HealthChecker>,
        shutdown: Arc<ShutdownCoordinator>,
        model_registry: Arc<ModelRegistry>,
        queue: Arc<RequestQueue>,
    ) -> Self {
        Self {
            health,
            shutdown,
            model_registry,
            queue,
        }
    }

    /// Handle a health check request. Returns appropriate response.
    pub async fn handle(&self, check_type: HealthCheckType) -> HealthCheckResponse {
        let shutdown_state = self.shutdown.state().await;
        let models = self.model_registry.count().await;
        let queue_len = self.queue.len().await;

        match check_type {
            HealthCheckType::Liveness => self.liveness_response(),
            HealthCheckType::Readiness => {
                self.readiness_response(shutdown_state, models, queue_len)
            }
            HealthCheckType::Full => self.full_response(shutdown_state, models, queue_len).await,
        }
    }

    fn liveness_response(&self) -> HealthCheckResponse {
        HealthCheckResponse {
            check_type: HealthCheckType::Liveness,
            ok: self.health.is_alive(),
            report: None,
        }
    }

    fn readiness_response(
        &self,
        shutdown_state: crate::shutdown::ShutdownState,
        models: usize,
        queue_len: usize,
    ) -> HealthCheckResponse {
        HealthCheckResponse {
            check_type: HealthCheckType::Readiness,
            ok: self.health.is_ready(shutdown_state, models, queue_len),
            report: None,
        }
    }

    async fn full_response(
        &self,
        shutdown_state: crate::shutdown::ShutdownState,
        models: usize,
        queue_len: usize,
    ) -> HealthCheckResponse {
        let memory = self.model_registry.total_memory().await;
        let report = self
            .health
            .report(shutdown_state, models, memory, queue_len);
        HealthCheckResponse {
            check_type: HealthCheckType::Full,
            ok: report.ready,
            report: Some(report),
        }
    }
}
