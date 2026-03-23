//! Security tests for sandbox escape prevention.
//!
//! Tests that sandbox boundaries are properly enforced and escape attempts fail.

use gg_core::sandbox::{SandboxConfig, create_sandbox};
use gg_core::memory::{ResourceLimits, ResourceLimitsConfig};

#[test]
fn sandbox_config_defaults_enforced() {
    let config = SandboxConfig::default();

    // Default should enforce 2GB memory limit
    assert_eq!(config.max_memory_bytes, 2 * 1024 * 1024 * 1024);

    // Default should enforce 30s CPU limit
    assert_eq!(config.max_cpu_time_ms, 30_000);

    // Sandbox should be enabled by default
    assert!(config.enabled);
}

#[test]
fn sandbox_can_be_created() {
    let config = SandboxConfig::default();
    let sandbox = create_sandbox(config);

    // Sandbox creation should succeed
    assert!(!sandbox.is_active()); // Not applied yet
}

#[test]
fn sandbox_disabled_is_noop() {
    let config = SandboxConfig {
        max_memory_bytes: 1024,
        max_cpu_time_ms: 1000,
        enabled: false,
        gpu_enabled: false,
    };
    let sandbox = create_sandbox(config);

    // Should still be creatable when disabled
    assert!(!sandbox.is_active());
}

#[test]
fn resource_limits_reject_exceeding_per_call() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024, // 1MB
        max_total_memory: 10 * 1024 * 1024, // 10MB
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);

    // Request exceeding per-call limit should fail
    let result = limits.try_acquire(2 * 1024 * 1024); // 2MB > 1MB
    assert!(result.is_err());

    // Verify error message
    match result {
        Err(err) => assert!(err.to_string().contains("Memory limit exceeded")),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn resource_limits_reject_exceeding_total() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 5 * 1024 * 1024, // 5MB
        max_total_memory: 8 * 1024 * 1024, // 8MB
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);

    // First allocation should succeed
    let guard1 = limits.try_acquire(5 * 1024 * 1024);
    assert!(guard1.is_ok());

    // Second allocation should fail (5MB + 5MB > 8MB total)
    let guard2 = limits.try_acquire(5 * 1024 * 1024);
    assert!(guard2.is_err());
}

#[test]
fn resource_limits_enforce_concurrency() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024 * 1024,
        max_total_memory: 10 * 1024 * 1024 * 1024,
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);

    // First two requests should succeed
    let guard1 = limits.try_acquire(1024);
    assert!(guard1.is_ok());
    let guard2 = limits.try_acquire(1024);
    assert!(guard2.is_ok());

    // Third should fail (concurrent limit = 2)
    let guard3 = limits.try_acquire(1024);
    assert!(guard3.is_err());

    // Verify error message
    match guard3 {
        Err(err) => assert!(err.to_string().contains("Queue full")),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn resource_guard_releases_on_drop() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024,
        max_total_memory: 2 * 1024 * 1024,
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);

    // Acquire and release via scope
    {
        let _guard = limits.try_acquire(1024 * 1024).unwrap();
        assert_eq!(limits.current_memory(), 1024 * 1024);
        assert_eq!(limits.current_concurrent(), 1);
    }

    // After drop, resources should be released
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);
}

#[test]
fn resource_limits_track_current_usage() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 10 * 1024 * 1024,
        max_total_memory: 100 * 1024 * 1024,
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);

    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);

    let _guard1 = limits.try_acquire(1 * 1024 * 1024).unwrap();
    assert_eq!(limits.current_memory(), 1 * 1024 * 1024);
    assert_eq!(limits.current_concurrent(), 1);

    let _guard2 = limits.try_acquire(2 * 1024 * 1024).unwrap();
    assert_eq!(limits.current_memory(), 3 * 1024 * 1024);
    assert_eq!(limits.current_concurrent(), 2);
}
