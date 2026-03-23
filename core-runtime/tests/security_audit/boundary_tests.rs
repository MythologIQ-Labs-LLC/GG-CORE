//! Sandbox Boundary Penetration Tests
//!
//! These tests validate that the sandbox boundaries are properly enforced
//! and cannot be escaped through various attack vectors.

use gg_core::memory::{ResourceLimits, ResourceLimitsConfig};
use gg_core::sandbox::{create_sandbox, Sandbox, SandboxConfig};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Attempt to exhaust memory by requesting more than allowed per-call limit.
#[test]
fn memory_boundary_reject_over_limit() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024,
        max_total_memory: 10 * 1024 * 1024,
        max_concurrent: 4,
    };
    let limits = ResourceLimits::new(config);
    let result = limits.try_acquire(2 * 1024 * 1024);
    assert!(result.is_err(), "Should reject allocation over per-call limit");
}

/// Attempt to exhaust total memory through multiple allocations.
#[test]
fn memory_boundary_total_exhaustion() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 5 * 1024 * 1024,
        max_total_memory: 8 * 1024 * 1024,
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);
    let guard1 = limits.try_acquire(5 * 1024 * 1024);
    assert!(guard1.is_ok(), "First allocation should succeed");
    let guard2 = limits.try_acquire(5 * 1024 * 1024);
    assert!(guard2.is_err(), "Second allocation should fail");
    assert_eq!(limits.current_memory(), 5 * 1024 * 1024);
}

/// Verify that resource guards properly release memory on drop.
#[test]
fn memory_boundary_release_on_drop() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 10 * 1024 * 1024,
        max_total_memory: 10 * 1024 * 1024,
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);
    {
        let _guard = limits.try_acquire(5 * 1024 * 1024).unwrap();
        assert_eq!(limits.current_memory(), 5 * 1024 * 1024);
    }
    assert_eq!(limits.current_memory(), 0);
    let result = limits.try_acquire(5 * 1024 * 1024);
    assert!(result.is_ok(), "Should be able to allocate after release");
}

/// Verify concurrent request limits are enforced.
#[test]
fn concurrency_limit_enforced() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024 * 1024,
        max_total_memory: 10 * 1024 * 1024 * 1024,
        max_concurrent: 2,
    };
    let limits = ResourceLimits::new(config);
    let guard1 = limits.try_acquire(1024);
    let guard2 = limits.try_acquire(1024);
    assert!(guard1.is_ok() && guard2.is_ok());
    let guard3 = limits.try_acquire(1024);
    assert!(guard3.is_err());
}

/// Verify concurrency counter decrements on guard drop.
#[test]
fn concurrency_release_on_drop() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024,
        max_total_memory: 1024 * 1024,
        max_concurrent: 1,
    };
    let limits = ResourceLimits::new(config);
    {
        let _guard = limits.try_acquire(100).unwrap();
        assert_eq!(limits.current_concurrent(), 1);
        assert!(limits.try_acquire(100).is_err());
    }
    assert_eq!(limits.current_concurrent(), 0);
    assert!(limits.try_acquire(100).is_ok());
}

/// Verify sandbox defaults enforce security limits.
#[test]
fn sandbox_config_defaults_secure() {
    let config = SandboxConfig::default();
    assert!(config.max_memory_bytes > 0);
    assert_eq!(config.max_memory_bytes, 2 * 1024 * 1024 * 1024);
    assert!(config.max_cpu_time_ms > 0);
    assert_eq!(config.max_cpu_time_ms, 30_000);
    assert!(config.enabled);
}

/// Verify sandbox can be created with custom limits.
#[test]
fn sandbox_custom_limits() {
    let config = SandboxConfig {
        max_memory_bytes: 512 * 1024 * 1024,
        max_cpu_time_ms: 5_000,
        enabled: true,
        gpu_enabled: false,
    };
    let sandbox = create_sandbox(config);
    assert!(!sandbox.is_active());
}

/// Verify disabled sandbox does not apply restrictions.
#[test]
fn sandbox_disabled_no_restrictions() {
    let config = SandboxConfig {
        max_memory_bytes: 1024,
        max_cpu_time_ms: 100,
        enabled: false,
        gpu_enabled: false,
    };
    let sandbox = create_sandbox(config);
    let result = sandbox.apply();
    assert!(result.success);
    assert!(result.error.is_some());
    assert!(result.error.unwrap().contains("disabled"));
}

/// Attempt rapid-fire resource requests (DoS simulation).
#[test]
fn resource_exhaustion_rapid_fire() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024,
        max_total_memory: 1024 * 1024,
        max_concurrent: 100,
    };
    let limits = ResourceLimits::new(config);
    let start = Instant::now();
    let mut success_count = 0;
    for _ in 0..10000 {
        if limits.try_acquire(512).is_ok() { success_count += 1; }
    }
    let elapsed = start.elapsed();
    assert!(success_count > 9900);
    assert!(elapsed < Duration::from_secs(1));
}

/// Attempt to hold resources indefinitely (lock-up attack simulation).
#[test]
fn resource_lockup_concurrent() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024,
        max_total_memory: 2 * 1024 * 1024,
        max_concurrent: 2,
    };
    let limits = Arc::new(ResourceLimits::new(config));
    let guard1 = limits.try_acquire(1024 * 1024).unwrap();
    let guard2 = limits.try_acquire(1024 * 1024).unwrap();
    assert!(limits.try_acquire(1).is_err());
    assert_eq!(limits.current_concurrent(), 2);
    assert_eq!(limits.current_memory(), 2 * 1024 * 1024);
    drop(guard1);
    drop(guard2);
    assert_eq!(limits.current_concurrent(), 0);
    assert!(limits.try_acquire(1024).is_ok());
}

/// Verify zero-byte allocation is handled.
#[test]
fn zero_byte_allocation() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024,
        max_total_memory: 1024 * 1024,
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);
    let result = limits.try_acquire(0);
    assert!(result.is_ok());
    assert_eq!(limits.current_concurrent(), 1);
    assert_eq!(limits.current_memory(), 0);
}

/// Test allocation at exactly the limit.
#[test]
fn allocation_at_exact_limit() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024,
        max_total_memory: 1024,
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);
    let result = limits.try_acquire(1024);
    assert!(result.is_ok());
}

/// Test allocation one byte over limit.
#[test]
fn allocation_one_over_limit() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 1024,
        max_total_memory: 1024 * 1024,
        max_concurrent: 10,
    };
    let limits = ResourceLimits::new(config);
    let result = limits.try_acquire(1025);
    assert!(result.is_err());
}

/// Test maximum possible values do not cause overflow.
#[test]
fn max_value_no_overflow() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: usize::MAX / 2,
        max_total_memory: usize::MAX,
        max_concurrent: usize::MAX,
    };
    let limits = ResourceLimits::new(config);
    let result = limits.try_acquire(usize::MAX / 4);
    let _ = result;
}

/// Verify sandbox usage tracking works.
#[test]
fn sandbox_usage_tracking() {
    let config = SandboxConfig {
        max_memory_bytes: 1024 * 1024 * 1024,
        max_cpu_time_ms: 60_000,
        enabled: true,
        gpu_enabled: false,
    };
    let sandbox = create_sandbox(config);
    if !sandbox.is_active() {
        if let Some(u) = sandbox.get_usage() {
            assert_eq!(u.memory_bytes, 0);
            assert_eq!(u.cpu_time_ms, 0);
        }
    }
}

/// Verify resource limits are thread-safe.
#[test]
fn resource_limits_thread_safe() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 100,
        max_total_memory: 1000,
        max_concurrent: 50,
    };
    let limits = Arc::new(ResourceLimits::new(config));
    let mut handles = vec![];
    for _ in 0..20 {
        let limits = Arc::clone(&limits);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                if let Ok(_guard) = limits.try_acquire(10) {
                    thread::sleep(Duration::from_micros(10));
                }
            }
        }));
    }
    for handle in handles { handle.join().unwrap(); }
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);
}

/// Attempt to bypass memory limits through concurrent requests.
#[test]
fn memory_boundary_concurrent_exhaustion() {
    let config = ResourceLimitsConfig {
        max_memory_per_call: 2 * 1024 * 1024,
        max_total_memory: 4 * 1024 * 1024,
        max_concurrent: 10,
    };
    let limits = Arc::new(ResourceLimits::new(config));
    let mut handles = vec![];
    for _ in 0..5 {
        let limits = Arc::clone(&limits);
        handles.push(thread::spawn(move || {
            limits.try_acquire(2 * 1024 * 1024)
        }));
    }
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let successes = results.iter().filter(|r| r.is_ok()).count();
    assert!(successes <= 2);
}
