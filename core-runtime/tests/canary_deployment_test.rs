//! Integration tests for canary deployment traffic splitting.
//!
//! Tests cover traffic weight distribution and gradual rollout progression.

use std::collections::BTreeMap;
use std::time::Duration;

use gg_core::ab_testing::{
    TrafficConfig, TrafficSplitter, VariantLabel, VariantMetrics,
};

// === Traffic Weight Distribution Tests ===

#[test]
fn canary_traffic_90_10_distribution() {
    let canary = VariantLabel::new("model-v2-canary");
    let splitter = TrafficSplitter::new(TrafficConfig::canary(canary.clone())).unwrap();

    let mut control_count = 0;
    let mut canary_count = 0;
    for i in 0..1000 {
        let variant = splitter.select(Some(&format!("session-{}", i)));
        if variant == &VariantLabel::control() {
            control_count += 1;
        } else if variant == &canary {
            canary_count += 1;
        }
    }

    // Expect ~90% control, ~10% canary (with hash distribution variance)
    assert!(control_count > 800, "Control got {} (expected ~900)", control_count);
    assert!(canary_count > 50 && canary_count < 200, "Canary got {}", canary_count);
}

#[test]
fn canary_traffic_80_20_distribution() {
    let canary = VariantLabel::new("v2-canary");
    let mut weights = BTreeMap::new();
    weights.insert(VariantLabel::control(), 80);
    weights.insert(canary.clone(), 20);
    let config = TrafficConfig { weights, sticky_sessions: true };
    let splitter = TrafficSplitter::new(config).unwrap();

    let mut canary_count = 0;
    for i in 0..1000 {
        if splitter.select(Some(&format!("sess-{}", i))) == &canary {
            canary_count += 1;
        }
    }

    assert!(canary_count > 100 && canary_count < 350, "Canary got {}", canary_count);
}

#[test]
fn canary_gradual_rollout_progression() {
    let canary = VariantLabel::new("new-model");
    let stages = [(95, 5), (90, 10), (80, 20), (70, 30), (50, 50)];

    for (control_weight, canary_weight) in stages {
        let mut weights = BTreeMap::new();
        weights.insert(VariantLabel::control(), control_weight);
        weights.insert(canary.clone(), canary_weight);
        let config = TrafficConfig { weights, sticky_sessions: true };
        let splitter = TrafficSplitter::new(config).unwrap();

        let mut canary_count = 0;
        for i in 0..500 {
            if splitter.select(Some(&format!("req-{}", i))) == &canary {
                canary_count += 1;
            }
        }

        let expected_min = (canary_weight as usize * 500 / 100).saturating_sub(100);
        let expected_max = canary_weight as usize * 500 / 100 + 100;
        assert!(
            canary_count >= expected_min && canary_count <= expected_max,
            "Stage {}%: canary={}, expected {}-{}",
            canary_weight,
            canary_count,
            expected_min,
            expected_max
        );
    }
}

#[test]
fn canary_sticky_session_consistency() {
    let canary = VariantLabel::new("canary-v3");
    let splitter = TrafficSplitter::new(TrafficConfig::canary(canary)).unwrap();
    let session = "user-12345-abc";

    let first_variant = splitter.select(Some(session));
    for _ in 0..100 {
        assert_eq!(splitter.select(Some(session)), first_variant);
    }
}

#[test]
fn canary_control_vs_canary_comparison() {
    let metrics = VariantMetrics::new();
    let control = VariantLabel::control();
    let canary = VariantLabel::new("canary-v2");
    let splitter = TrafficSplitter::new(TrafficConfig::canary(canary.clone())).unwrap();

    for i in 0..200 {
        let variant = splitter.select(Some(&format!("req-{}", i)));
        metrics.get_or_create(variant).record_request();
        let latency = if variant == &canary { 40 } else { 50 };
        metrics.get_or_create(variant).record_success(Duration::from_millis(latency), 20);
    }

    let snapshots = metrics.all_snapshots();
    let control_stats = snapshots.get(&control).unwrap();
    let canary_stats = snapshots.get(&canary).unwrap();

    // Canary should have better latency
    assert!(canary_stats.avg_latency_ms < control_stats.avg_latency_ms);
    // Control should have more traffic (~90%)
    assert!(control_stats.requests > canary_stats.requests * 3);
}

#[test]
fn canary_traffic_routing_accuracy() {
    let canary = VariantLabel::new("accurate-canary");
    let mut weights = BTreeMap::new();
    weights.insert(VariantLabel::control(), 75);
    weights.insert(canary.clone(), 25);
    let config = TrafficConfig { weights, sticky_sessions: true };
    let splitter = TrafficSplitter::new(config).unwrap();

    let mut distribution = std::collections::HashMap::new();
    for i in 0..2000 {
        let variant = splitter.select(Some(&format!("user-{}", i)));
        *distribution.entry(variant.as_str().to_string()).or_insert(0) += 1;
    }

    let control_pct = distribution.get("control").unwrap_or(&0) * 100 / 2000;
    let canary_pct = distribution.get("accurate-canary").unwrap_or(&0) * 100 / 2000;

    // Allow 10% variance from expected
    assert!(control_pct >= 65 && control_pct <= 85, "Control: {}%", control_pct);
    assert!(canary_pct >= 15 && canary_pct <= 35, "Canary: {}%", canary_pct);
}
