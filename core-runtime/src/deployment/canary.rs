// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Canary deployment controller.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use super::metrics::{compare_metrics, MetricsCollector, MetricsSnapshot};
use super::thresholds::{analyze_thresholds, AlertLevel, AnalysisThresholds, ThresholdResult};

/// Canary deployment phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanaryPhase {
    Initializing,
    Progressing { weight: u8 },
    Analyzing,
    Promoting,
    RollingBack,
    Complete,
    Failed,
}

impl CanaryPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(self, CanaryPhase::Complete | CanaryPhase::Failed)
    }

    pub fn allows_traffic(&self) -> bool {
        matches!(
            self,
            CanaryPhase::Progressing { .. } | CanaryPhase::Analyzing
        )
    }
}

/// Canary deployment state.
#[derive(Debug, Clone)]
pub struct CanaryState {
    pub phase: CanaryPhase,
    pub traffic_weight: u8,
    pub analysis_cycles: u32,
    pub last_result: Option<ThresholdResult>,
    pub started_at: Instant,
    pub phase_changed_at: Instant,
}

impl Default for CanaryState {
    fn default() -> Self {
        Self {
            phase: CanaryPhase::Initializing,
            traffic_weight: 0,
            analysis_cycles: 0,
            last_result: None,
            started_at: Instant::now(),
            phase_changed_at: Instant::now(),
        }
    }
}

impl CanaryState {
    pub fn time_in_phase(&self) -> Duration {
        self.phase_changed_at.elapsed()
    }

    pub fn total_duration(&self) -> Duration {
        self.started_at.elapsed()
    }
}

/// Configuration for canary controller.
#[derive(Debug, Clone)]
pub struct CanaryConfig {
    pub thresholds: AnalysisThresholds,
    pub weight_steps: Vec<u8>,
    pub step_duration: Duration,
    pub max_duration: Duration,
    pub max_cycles: u32,
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            thresholds: AnalysisThresholds::default(),
            weight_steps: vec![5, 10, 25, 50, 100],
            step_duration: Duration::from_secs(300),
            max_duration: Duration::from_secs(3600),
            max_cycles: 12,
        }
    }
}

/// Canary deployment controller.
pub struct CanaryController {
    config: CanaryConfig,
    state: CanaryState,
    canary_metrics: MetricsCollector,
    stable_metrics: MetricsCollector,
    current_step: usize,
}

impl CanaryController {
    pub fn new(config: CanaryConfig) -> Self {
        Self {
            config,
            state: CanaryState::default(),
            canary_metrics: MetricsCollector::new(10000),
            stable_metrics: MetricsCollector::new(10000),
            current_step: 0,
        }
    }

    pub fn state(&self) -> &CanaryState {
        &self.state
    }
    pub fn traffic_weight(&self) -> u8 {
        self.state.traffic_weight
    }

    pub fn record_canary(&mut self, latency: Duration, success: bool) {
        if success {
            self.canary_metrics.record_success(latency);
        } else {
            self.canary_metrics.record_error(latency);
        }
    }

    pub fn record_stable(&mut self, latency: Duration, success: bool) {
        if success {
            self.stable_metrics.record_success(latency);
        } else {
            self.stable_metrics.record_error(latency);
        }
    }

    pub fn start(&mut self) {
        self.transition_to(CanaryPhase::Progressing { weight: 0 });
        self.advance_weight();
    }

    pub fn analyze(&mut self) -> CanaryDecision {
        if self.state.phase.is_terminal() {
            return CanaryDecision::Complete;
        }
        if self.state.total_duration() > self.config.max_duration {
            self.transition_to(CanaryPhase::Failed);
            return CanaryDecision::Timeout;
        }

        let canary_snap = self.canary_metrics.snapshot();
        let stable_snap = self.stable_metrics.snapshot();

        if !self.has_sufficient_samples(&canary_snap, &stable_snap) {
            return CanaryDecision::InsufficientData;
        }

        let comparison = compare_metrics(
            &canary_snap,
            &stable_snap,
            self.config.thresholds.confidence_level,
        );
        let result = analyze_thresholds(&comparison, &self.config.thresholds);
        self.state.last_result = Some(result.clone());
        self.state.analysis_cycles += 1;
        self.make_decision(result)
    }

    fn has_sufficient_samples(&self, c: &MetricsSnapshot, s: &MetricsSnapshot) -> bool {
        let min = self.config.thresholds.min_sample_size;
        c.has_sufficient_samples(min) && s.has_sufficient_samples(min)
    }

    fn make_decision(&mut self, result: ThresholdResult) -> CanaryDecision {
        if result.should_rollback() {
            self.transition_to(CanaryPhase::RollingBack);
            return CanaryDecision::Rollback(result.reason);
        }
        if result.should_promote() {
            if self.can_advance_weight() {
                self.advance_weight();
                return CanaryDecision::AdvanceWeight(self.state.traffic_weight);
            }
            self.transition_to(CanaryPhase::Promoting);
            return CanaryDecision::Promote;
        }
        if self.state.analysis_cycles >= self.config.max_cycles {
            if result.alert_level == AlertLevel::Warning {
                self.transition_to(CanaryPhase::Promoting);
                return CanaryDecision::Promote;
            }
            self.transition_to(CanaryPhase::RollingBack);
            return CanaryDecision::Rollback("Max cycles reached".to_string());
        }
        CanaryDecision::Continue
    }

    fn can_advance_weight(&self) -> bool {
        self.current_step < self.config.weight_steps.len() - 1
    }

    fn advance_weight(&mut self) {
        if self.current_step < self.config.weight_steps.len() {
            self.state.traffic_weight = self.config.weight_steps[self.current_step];
            self.current_step += 1;
            self.canary_metrics.reset();
            self.stable_metrics.reset();
        }
    }

    fn transition_to(&mut self, phase: CanaryPhase) {
        self.state.phase = phase;
        self.state.phase_changed_at = Instant::now();
    }

    pub fn complete(&mut self) {
        self.transition_to(CanaryPhase::Complete);
        self.state.traffic_weight = 100;
    }

    pub fn fail(&mut self) {
        self.transition_to(CanaryPhase::Failed);
        self.state.traffic_weight = 0;
    }
}

/// Decision from canary analysis.
#[derive(Debug, Clone)]
pub enum CanaryDecision {
    InsufficientData,
    Continue,
    AdvanceWeight(u8),
    Promote,
    Rollback(String),
    Complete,
    Timeout,
}

impl CanaryDecision {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            CanaryDecision::Promote
                | CanaryDecision::Rollback(_)
                | CanaryDecision::Complete
                | CanaryDecision::Timeout
        )
    }
}
