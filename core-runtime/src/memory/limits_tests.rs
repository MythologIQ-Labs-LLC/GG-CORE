//! Resource limit unit tests (moved out of limits.rs for the Section 4 Razor).

use super::*;

fn config_1mb_2concurrent() -> ResourceLimitsConfig {
    ResourceLimitsConfig {
        max_memory_per_call: 1024 * 1024,
        max_total_memory: 4 * 1024 * 1024,
        max_concurrent: 2,
    }
}

#[test]
fn acquire_within_limits_succeeds() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    let guard = limits.try_acquire(512 * 1024);
    assert!(guard.is_ok());
    assert_eq!(limits.current_memory(), 512 * 1024);
    assert_eq!(limits.current_concurrent(), 1);
}

#[test]
fn per_call_memory_cap_rejects_oversized_request() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    let err = limits.try_acquire(2 * 1024 * 1024).unwrap_err();
    assert!(
        matches!(err, InferenceError::MemoryExceeded { .. }),
        "expected MemoryExceeded, got {err}"
    );
    // Gauges must not have changed.
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);
}

#[test]
fn total_memory_cap_rejects_when_pool_full() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 3 * 1024 * 1024,
        max_total_memory: 4 * 1024 * 1024,
        max_concurrent: 8,
    });
    // First acquire: 3 MiB — fits in total.
    let _g1 = limits.try_acquire(3 * 1024 * 1024).unwrap();
    // Second acquire: 2 MiB — 3+2 = 5 MiB > 4 MiB total.
    let err = limits.try_acquire(2 * 1024 * 1024).unwrap_err();
    assert!(matches!(err, InferenceError::MemoryExceeded { .. }));
    // Total memory reflects only the first allocation.
    assert_eq!(limits.current_memory(), 3 * 1024 * 1024);
}

#[test]
fn concurrency_cap_rejects_when_slots_exhausted() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: usize::MAX,
        max_total_memory: usize::MAX,
        max_concurrent: 1,
    });
    let _g = limits.try_acquire(0).unwrap();
    let err = limits.try_acquire(0).unwrap_err();
    assert!(
        matches!(err, InferenceError::QueueFull { .. }),
        "expected QueueFull, got {err}"
    );
    assert_eq!(limits.current_concurrent(), 1);
}

#[test]
fn guard_drop_releases_memory_and_concurrency() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    {
        let _guard = limits.try_acquire(1024).unwrap();
        assert_eq!(limits.current_memory(), 1024);
        assert_eq!(limits.current_concurrent(), 1);
    }
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);
}

#[test]
fn max_memory_per_call_accessor_matches_config() {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 42 * 1024,
        max_total_memory: usize::MAX,
        max_concurrent: 1,
    });
    assert_eq!(limits.max_memory_per_call(), 42 * 1024);
}

#[test]
fn memory_cap_100_percent_rejection() {
    // max_memory_per_call = 0 means every request is rejected.
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 0,
        max_total_memory: usize::MAX,
        max_concurrent: 8,
    });
    for _ in 0..5 {
        let err = limits.try_acquire(1).unwrap_err();
        assert!(matches!(err, InferenceError::MemoryExceeded { .. }));
    }
    assert_eq!(limits.current_memory(), 0);
    assert_eq!(limits.current_concurrent(), 0);
}

#[test]
fn working_reservation_without_ceiling_is_bounded_only_by_total() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    let _g = limits
        .try_acquire_working(3 * 1024 * 1024)
        .expect("no ceiling: 3 MiB fits the 4 MiB total");
    assert!(
        limits.try_acquire_working(2 * 1024 * 1024).is_err(),
        "total still binds"
    );
    assert_eq!(limits.per_call_ceiling(), None);
}

#[test]
fn working_reservation_above_an_explicit_ceiling_is_refused_without_side_effects() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    limits.set_per_call_ceiling(2 * 1024 * 1024).unwrap();
    let err = limits.try_acquire_working(3 * 1024 * 1024).unwrap_err();
    assert!(
        matches!(err, InferenceError::MemoryExceeded { limit, .. } if limit == 2 * 1024 * 1024)
    );
    assert_eq!(
        (limits.current_memory(), limits.current_concurrent()),
        (0, 0)
    );
    assert!(limits.try_acquire_working(2 * 1024 * 1024).is_ok());
}

#[test]
fn per_call_ceiling_is_set_once_and_shared_by_clones() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    let clone = limits.clone();
    limits.set_per_call_ceiling(1024).unwrap();
    assert_eq!(
        clone.per_call_ceiling(),
        Some(1024),
        "clones share the ceiling"
    );
    assert_eq!(
        limits.set_per_call_ceiling(2048),
        Err(2048),
        "set at most once"
    );
}

#[test]
fn legacy_try_acquire_keeps_the_config_per_call_cap() {
    let limits = ResourceLimits::new(config_1mb_2concurrent());
    assert!(limits.try_acquire(2 * 1024 * 1024).is_err());
}
