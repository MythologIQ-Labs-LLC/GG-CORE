// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Deployment automation for canary analysis.
//!
//! Provides metrics collection, statistical analysis, and canary state management
//! for automated deployment decisions.

pub mod canary;
pub mod metrics;
pub mod thresholds;

pub use canary::{CanaryConfig, CanaryController, CanaryDecision, CanaryPhase, CanaryState};
pub use metrics::{
    compare_metrics, DeploymentMetrics, MetricsCollector, MetricsSnapshot, StatisticalComparison,
};
pub use thresholds::{analyze_thresholds, AlertLevel, AnalysisThresholds, ThresholdResult};
