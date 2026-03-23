//! Resource limit enforcement for CORE Runtime.
//!
//! Tracks and enforces memory and concurrency limits per inference call.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::engine::InferenceError;

/// Configuration for resource limits.
#[derive(Debug, Clone)]
pub struct ResourceLimitsConfig {
    /// Maximum memory per inference call (bytes).
    pub max_memory_per_call: usize,
    /// Maximum total memory across all calls (bytes).
    pub max_total_memory: usize,
    /// Maximum concurrent inference requests.
    pub max_concurrent: usize,
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_per_call: 1024 * 1024 * 1024, // 1GB
            max_total_memory: 2 * 1024 * 1024 * 1024, // 2GB
            max_concurrent: 2,
        }
    }
}

/// Shared state for resource tracking.
struct LimitsInner {
    config: ResourceLimitsConfig,
    current_memory: AtomicUsize,
    current_concurrent: AtomicUsize,
}

/// Resource limit tracker and enforcer.
#[derive(Clone)]
pub struct ResourceLimits {
    inner: Arc<LimitsInner>,
}

impl ResourceLimits {
    /// Create a new resource limits tracker.
    pub fn new(config: ResourceLimitsConfig) -> Self {
        Self {
            inner: Arc::new(LimitsInner {
                config,
                current_memory: AtomicUsize::new(0),
                current_concurrent: AtomicUsize::new(0),
            }),
        }
    }

    /// Try to acquire resources for an inference call.
    pub fn try_acquire(&self, memory_bytes: usize) -> Result<ResourceGuard, InferenceError> {
        let inner = &self.inner;

        // Check memory limit
        if memory_bytes > inner.config.max_memory_per_call {
            return Err(InferenceError::MemoryExceeded {
                used: memory_bytes,
                limit: inner.config.max_memory_per_call,
            });
        }

        // Try to reserve memory
        let prev_memory = inner.current_memory.fetch_add(memory_bytes, Ordering::SeqCst);
        if prev_memory + memory_bytes > inner.config.max_total_memory {
            inner.current_memory.fetch_sub(memory_bytes, Ordering::SeqCst);
            return Err(InferenceError::MemoryExceeded {
                used: prev_memory + memory_bytes,
                limit: inner.config.max_total_memory,
            });
        }

        // Try to reserve concurrency slot
        let prev_concurrent = inner.current_concurrent.fetch_add(1, Ordering::SeqCst);
        if prev_concurrent >= inner.config.max_concurrent {
            inner.current_concurrent.fetch_sub(1, Ordering::SeqCst);
            inner.current_memory.fetch_sub(memory_bytes, Ordering::SeqCst);
            return Err(InferenceError::QueueFull {
                current: prev_concurrent + 1,
                max: inner.config.max_concurrent,
            });
        }

        Ok(ResourceGuard {
            memory_bytes,
            inner: self.inner.clone(),
        })
    }

    /// Current memory usage in bytes.
    pub fn current_memory(&self) -> usize {
        self.inner.current_memory.load(Ordering::SeqCst)
    }

    /// Current number of concurrent requests.
    pub fn current_concurrent(&self) -> usize {
        self.inner.current_concurrent.load(Ordering::SeqCst)
    }

    /// Maximum memory allowed per inference call (bytes).
    pub fn max_memory_per_call(&self) -> usize {
        self.inner.config.max_memory_per_call
    }
}

/// RAII guard that releases resources when dropped.
pub struct ResourceGuard {
    memory_bytes: usize,
    inner: Arc<LimitsInner>,
}

impl std::fmt::Debug for ResourceGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceGuard")
            .field("memory_bytes", &self.memory_bytes)
            .finish()
    }
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.inner.current_memory.fetch_sub(self.memory_bytes, Ordering::SeqCst);
        self.inner.current_concurrent.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
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
}
