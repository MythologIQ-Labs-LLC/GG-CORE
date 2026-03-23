//! Integration tests for canary deployment metrics and rollback.
//!
//! Tests cover error rate calculation, threshold detection, and rollback scenarios.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use gg_core::ab_testing::{TrafficConfig, TrafficSplitter, VariantLabel, VariantMetrics};

// === Metrics Analysis Tests ===

#[test]
fn canary_error_rate_calculation() {
    let metrics = VariantMetrics::new();
    let canary = VariantLabel::new("canary-v1");

    // Simulate 100 requests with 5% error rate
    for i in 0..100 {
        metrics.get_or_create(&canary).record_request();
        if i % 20 == 0 {
            metrics.get_or_create(&canary).record_failure();
        } else {
            metrics.get_or_create(&canary).record_success(Duration::from_millis(50), 10);
        }
    }

    let snapshot = metrics.all_snapshots();
    let stats = snapshot.get(&canary).unwrap();
    assert_eq!(stats.requests, 100);
    assert_eq!(stats.failures, 5);
    assert!((stats.success_rate - 0.95).abs() < 0.01);
}

#[test]
fn canary_latency_percentile_tracking() {
    let metrics = VariantMetrics::new();
    let canary = VariantLabel::new("canary");

    let latencies = [10, 20, 30, 40, 50, 100, 200, 300, 400, 500];
    for latency in latencies {
        metrics.get_or_create(&canary).record_request();
        metrics.get_or_create(&canary).record_success(Duration::from_millis(latency), 10);
    }

    let snapshot = metrics.all_snapshots();
    let stats = snapshot.get(&canary).unwrap();
    assert_eq!(stats.requests, 10);
    assert_eq!(stats.successes, 10);
    // Average: (10+20+30+40+50+100+200+300+400+500)/10 = 165ms
    assert!(stats.avg_latency_ms > 100.0 && stats.avg_latency_ms < 200.0);
}

#[test]
fn canary_threshold_breach_detection() {
    let metrics = VariantMetrics::new();
    let canary = VariantLabel::new("canary-test");
    let error_threshold = 0.05; // 5% max error rate

    // Simulate increasing errors
    for i in 0..100 {
        metrics.get_or_create(&canary).record_request();
        if i < 90 {
            metrics.get_or_create(&canary).record_success(Duration::from_millis(30), 10);
        } else {
            metrics.get_or_create(&canary).record_failure();
        }
    }

    let snapshot = metrics.all_snapshots();
    let stats = snapshot.get(&canary).unwrap();
    let error_rate = 1.0 - stats.success_rate;
    let breach = error_rate > error_threshold;

    assert!(breach, "Error rate {} should breach threshold {}", error_rate, error_threshold);
}

// === Rollback Tests ===

#[test]
fn canary_automatic_rollback_on_threshold_breach() {
    let canary = VariantLabel::new("canary-bad");
    let error_threshold = 0.10;
    let mut current_canary_weight = 10u8;

    let metrics = VariantMetrics::new();
    for _ in 0..50 {
        metrics.get_or_create(&canary).record_request();
        metrics.get_or_create(&canary).record_failure(); // 100% failure
    }

    let snapshot = metrics.all_snapshots();
    let error_rate = 1.0 - snapshot.get(&canary).unwrap().success_rate;

    // Simulate automatic rollback
    if error_rate > error_threshold {
        current_canary_weight = 0;
    }

    assert_eq!(current_canary_weight, 0, "Should rollback to 0% canary traffic");
}

#[test]
fn canary_manual_rollback_traffic_shift() {
    let canary = VariantLabel::new("canary-v1");

    // Before rollback: 90/10
    let mut weights = BTreeMap::new();
    weights.insert(VariantLabel::control(), 90);
    weights.insert(canary.clone(), 10);
    let before = TrafficConfig { weights, sticky_sessions: true };
    assert!(before.validate().is_ok());

    // After rollback: 100/0
    let mut weights = BTreeMap::new();
    weights.insert(VariantLabel::control(), 100);
    weights.insert(canary.clone(), 0);
    let after = TrafficConfig { weights, sticky_sessions: true };
    assert!(after.validate().is_ok());

    let splitter = TrafficSplitter::new(after).unwrap();
    for i in 0..100 {
        assert_eq!(splitter.select(Some(&format!("s-{}", i))), &VariantLabel::control());
    }
}

#[test]
fn canary_state_preservation_during_rollback() {
    let metrics = VariantMetrics::new();
    let control = VariantLabel::control();
    let canary = VariantLabel::new("canary");

    // Accumulate metrics before rollback
    for _ in 0..50 {
        metrics.get_or_create(&control).record_request();
        metrics.get_or_create(&control).record_success(Duration::from_millis(30), 10);
        metrics.get_or_create(&canary).record_request();
        metrics.get_or_create(&canary).record_failure();
    }

    // Simulate rollback (metrics should be preserved for analysis)
    let pre_rollback = metrics.all_snapshots();
    assert_eq!(pre_rollback.get(&control).unwrap().requests, 50);
    assert_eq!(pre_rollback.get(&canary).unwrap().failures, 50);

    // After rollback, all traffic to control
    for _ in 0..50 {
        metrics.get_or_create(&control).record_request();
        metrics.get_or_create(&control).record_success(Duration::from_millis(30), 10);
    }

    let post_rollback = metrics.all_snapshots();
    assert_eq!(post_rollback.get(&control).unwrap().requests, 100);
    // Canary metrics preserved
    assert_eq!(post_rollback.get(&canary).unwrap().failures, 50);
}

#[test]
fn canary_concurrent_metrics_recording() {
    let metrics = Arc::new(VariantMetrics::new());
    let canary = VariantLabel::new("canary-concurrent");
    let mut handles = vec![];

    for _ in 0..8 {
        let m = Arc::clone(&metrics);
        let c = canary.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                m.get_or_create(&c).record_request();
                m.get_or_create(&c).record_success(Duration::from_millis(25), 15);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let snapshot = metrics.all_snapshots();
    assert_eq!(snapshot.get(&canary).unwrap().requests, 800);
    assert_eq!(snapshot.get(&canary).unwrap().successes, 800);
}

#[test]
fn canary_rollback_timing_verification() {
    let canary = VariantLabel::new("canary-timing");
    let metrics = VariantMetrics::new();

    // Simulate gradual degradation
    for i in 0..100 {
        metrics.get_or_create(&canary).record_request();
        // Error rate increases over time
        if i > 80 {
            metrics.get_or_create(&canary).record_failure();
        } else {
            metrics.get_or_create(&canary).record_success(Duration::from_millis(50), 10);
        }

        // Check for rollback condition at each step
        let snapshot = metrics.all_snapshots();
        let stats = snapshot.get(&canary).unwrap();
        let error_rate = 1.0 - stats.success_rate;

        // Rollback threshold: 10%
        if error_rate > 0.10 && stats.requests >= 90 {
            // Rollback triggered at correct point
            assert!(i >= 90, "Rollback triggered too early at iteration {}", i);
            break;
        }
    }
}
