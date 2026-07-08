// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Analysis thresholds for canary deployment decisions.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::metrics::StatisticalComparison;

/// Configuration for analysis thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisThresholds {
    /// Maximum allowed error rate increase (e.g., 0.01 = 1%).
    pub max_error_rate_increase: f64,
    /// Maximum allowed P99 latency ratio (e.g., 1.2 = 20% increase).
    pub max_latency_p99_increase: f64,
    /// Maximum allowed throughput decrease ratio (e.g., 0.9 = 10% decrease).
    pub min_throughput_ratio: f64,
    /// Minimum sample size for statistical validity.
    pub min_sample_size: usize,
    /// Required confidence level (e.g., 0.95 = 95%).
    pub confidence_level: f64,
    /// Analysis window duration.
    pub analysis_window: Duration,
}

impl Default for AnalysisThresholds {
    fn default() -> Self {
        Self {
            max_error_rate_increase: 0.01,
            max_latency_p99_increase: 1.2,
            min_throughput_ratio: 0.9,
            min_sample_size: 1000,
            confidence_level: 0.95,
            analysis_window: Duration::from_secs(300),
        }
    }
}

impl AnalysisThresholds {
    /// Create strict thresholds for critical workloads.
    pub fn strict() -> Self {
        Self {
            max_error_rate_increase: 0.005,
            max_latency_p99_increase: 1.1,
            min_throughput_ratio: 0.95,
            min_sample_size: 2000,
            confidence_level: 0.99,
            analysis_window: Duration::from_secs(600),
        }
    }

    /// Create relaxed thresholds for non-critical workloads.
    pub fn relaxed() -> Self {
        Self {
            max_error_rate_increase: 0.02,
            max_latency_p99_increase: 1.5,
            min_throughput_ratio: 0.8,
            min_sample_size: 500,
            confidence_level: 0.90,
            analysis_window: Duration::from_secs(180),
        }
    }
}

/// Alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertLevel {
    None,
    Warning,
    Critical,
    AutoRollback,
}

impl AlertLevel {
    /// Check if this level requires immediate action.
    pub fn requires_action(&self) -> bool {
        matches!(self, AlertLevel::Critical | AlertLevel::AutoRollback)
    }

    /// Check if this level triggers automatic rollback.
    pub fn triggers_rollback(&self) -> bool {
        matches!(self, AlertLevel::AutoRollback)
    }
}

/// Result of threshold analysis.
#[derive(Debug, Clone)]
pub struct ThresholdResult {
    pub alert_level: AlertLevel,
    pub error_rate_level: AlertLevel,
    pub latency_level: AlertLevel,
    pub throughput_level: AlertLevel,
    pub reason: String,
}

impl ThresholdResult {
    pub fn should_promote(&self) -> bool {
        self.alert_level == AlertLevel::None
    }

    pub fn should_rollback(&self) -> bool {
        self.alert_level.triggers_rollback()
    }

    pub fn should_continue(&self) -> bool {
        !self.should_promote() && !self.should_rollback()
    }
}

/// Analyze metrics against thresholds.
pub fn analyze_thresholds(
    comparison: &StatisticalComparison,
    thresholds: &AnalysisThresholds,
) -> ThresholdResult {
    let error_rate_level = compute_error_level(
        comparison.error_rate_diff,
        thresholds.max_error_rate_increase,
    );
    let latency_level = compute_latency_level(
        comparison.latency_p99_ratio,
        thresholds.max_latency_p99_increase,
    );
    let throughput_level =
        compute_throughput_level(comparison.throughput_ratio, thresholds.min_throughput_ratio);

    let alert_level = max_level(&[error_rate_level, latency_level, throughput_level]);
    let reason = build_reason(
        &error_rate_level,
        &latency_level,
        &throughput_level,
        comparison,
    );

    ThresholdResult {
        alert_level,
        error_rate_level,
        latency_level,
        throughput_level,
        reason,
    }
}

fn compute_error_level(diff: f64, max_increase: f64) -> AlertLevel {
    if diff <= 0.0 || diff < max_increase * 0.5 {
        AlertLevel::None
    } else if diff < max_increase * 0.8 {
        AlertLevel::Warning
    } else if diff < max_increase {
        AlertLevel::Critical
    } else {
        AlertLevel::AutoRollback
    }
}

fn compute_latency_level(ratio: f64, max_ratio: f64) -> AlertLevel {
    let excess = ratio - 1.0;
    let threshold_excess = max_ratio - 1.0;

    if excess <= 0.0 || excess < threshold_excess * 0.5 {
        AlertLevel::None
    } else if excess < threshold_excess * 0.8 {
        AlertLevel::Warning
    } else if excess < threshold_excess {
        AlertLevel::Critical
    } else {
        AlertLevel::AutoRollback
    }
}

fn compute_throughput_level(ratio: f64, min_ratio: f64) -> AlertLevel {
    if ratio >= 1.0 || ratio > min_ratio + (1.0 - min_ratio) * 0.5 {
        AlertLevel::None
    } else if ratio > min_ratio + (1.0 - min_ratio) * 0.2 {
        AlertLevel::Warning
    } else if ratio > min_ratio {
        AlertLevel::Critical
    } else {
        AlertLevel::AutoRollback
    }
}

fn max_level(levels: &[AlertLevel]) -> AlertLevel {
    levels
        .iter()
        .max_by_key(|l| match l {
            AlertLevel::None => 0,
            AlertLevel::Warning => 1,
            AlertLevel::Critical => 2,
            AlertLevel::AutoRollback => 3,
        })
        .copied()
        .unwrap_or(AlertLevel::None)
}

fn build_reason(
    error: &AlertLevel,
    latency: &AlertLevel,
    throughput: &AlertLevel,
    comparison: &StatisticalComparison,
) -> String {
    let mut parts = Vec::new();
    if *error != AlertLevel::None {
        parts.push(format!(
            "error rate +{:.2}%",
            comparison.error_rate_diff * 100.0
        ));
    }
    if *latency != AlertLevel::None {
        parts.push(format!("P99 latency {:.1}x", comparison.latency_p99_ratio));
    }
    if *throughput != AlertLevel::None {
        parts.push(format!(
            "throughput {:.1}%",
            comparison.throughput_ratio * 100.0
        ));
    }
    if parts.is_empty() {
        "All metrics within thresholds".to_string()
    } else {
        parts.join(", ")
    }
}
