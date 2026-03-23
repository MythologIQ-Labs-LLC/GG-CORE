//! TDD-Light tests for resource limit enforcement.

use gg_core::engine::InferenceError;
use gg_core::memory::{ResourceLimits, ResourceLimitsConfig};

#[test]
fn limits_allow_within_bounds() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1000,
        max_total_memory: 2000,
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);

    let guard = limits.try_acquire(500);
    assert!(guard.is_ok());
}

#[test]
fn limits_reject_exceeds_per_call() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 100,
        max_total_memory: 1000,
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);

    let result = limits.try_acquire(200);
    assert!(matches!(
        result,
        Err(InferenceError::MemoryExceeded { used: 200, limit: 100 })
    ));
}

#[test]
fn limits_reject_exceeds_total() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1000,
        max_total_memory: 500,
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);

    let result = limits.try_acquire(600);
    assert!(matches!(result, Err(InferenceError::MemoryExceeded { .. })));
}

#[test]
fn limits_reject_exceeds_concurrent() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1000,
        max_total_memory: 10000,
        max_concurrent: 1,
    };
    let limits = ResourceLimits::new(config);

    let _guard1 = limits.try_acquire(100).unwrap();

    let result = limits.try_acquire(100);
    assert!(matches!(
        result,
        Err(InferenceError::QueueFull { current: 2, max: 1 })
    ));
}

#[test]
fn limits_release_on_drop() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1000,
        max_total_memory: 1000,
        max_concurrent: 1,
    };
    let limits = ResourceLimits::new(config);

    {
        let _guard = limits.try_acquire(500).unwrap();
        assert_eq!(limits.current_memory(), 500);
        assert_eq!(limits.current_concurrent(), 1);
    }

    // Guard dropped, resources released
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);
}

#[test]
fn limits_track_multiple_acquisitions() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 500,
        max_total_memory: 1000,
        max_concurrent: 5,
    };
    let limits = ResourceLimits::new(config);

    let _g1 = limits.try_acquire(200).unwrap();
    let _g2 = limits.try_acquire(300).unwrap();

    assert_eq!(limits.current_memory(), 500);
    assert_eq!(limits.current_concurrent(), 2);

    // Third acquisition should still fit
    let _g3 = limits.try_acquire(400).unwrap();
    assert_eq!(limits.current_memory(), 900);
}

#[test]
fn limits_clone_shares_state() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1000,
        max_total_memory: 2000,
        max_concurrent: 5,
    };
    let limits1 = ResourceLimits::new(config);
    let limits2 = limits1.clone();

    let _guard = limits1.try_acquire(500).unwrap();

    // Clone should see the same state
    assert_eq!(limits2.current_memory(), 500);
    assert_eq!(limits2.current_concurrent(), 1);
}

#[test]
fn limits_default_config_is_reasonable() {
    let config = ResourceLimitsConfig::default();

    assert!(config.max_memory_per_call >= 1024 * 1024 * 1024); // At least 1GB
    assert!(config.max_total_memory >= config.max_memory_per_call);
    assert!(config.max_concurrent >= 1);
}
