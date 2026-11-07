//! Resident (model-weight) memory accounting.
//!
//! Weights are charged once when a model is registered and released when it
//! is unloaded. They share the single `current_memory` counter with
//! per-request working reservations, so `resident + working <= total` holds
//! without a race between two counters. Resident reservations never take a
//! concurrency slot and are never compared against the per-call budget.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::{LimitsInner, ResourceLimits};
use crate::engine::InferenceError;

/// RAII reservation for a loaded model's resident bytes.
pub struct ResidentGuard {
    bytes: usize,
    inner: Arc<LimitsInner>,
}

impl std::fmt::Debug for ResidentGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResidentGuard")
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl Drop for ResidentGuard {
    fn drop(&mut self) {
        self.inner
            .current_memory
            .fetch_sub(self.bytes, Ordering::SeqCst);
    }
}

impl ResourceLimits {
    /// Reserve resident bytes for a model being registered. Fails with
    /// `MemoryExceeded` (limit = total budget) when it would not fit.
    pub fn reserve_resident(&self, bytes: usize) -> Result<ResidentGuard, InferenceError> {
        let inner = &self.inner;
        let prev = inner.current_memory.fetch_add(bytes, Ordering::SeqCst);
        let total = prev.saturating_add(bytes);
        if total > inner.config.max_total_memory {
            inner.current_memory.fetch_sub(bytes, Ordering::SeqCst);
            return Err(InferenceError::MemoryExceeded {
                used: total,
                limit: inner.config.max_total_memory,
            });
        }
        Ok(ResidentGuard {
            bytes,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Pre-load check (no reservation): would `bytes` fit right now?
    /// Lets a load be refused before any backend allocation.
    pub fn check_resident_headroom(&self, bytes: usize) -> Result<(), InferenceError> {
        let used = self.current_memory().saturating_add(bytes);
        if used > self.inner.config.max_total_memory {
            return Err(InferenceError::MemoryExceeded {
                used,
                limit: self.inner.config.max_total_memory,
            });
        }
        Ok(())
    }

    /// Total-memory budget (resident + working).
    pub fn max_total_memory(&self) -> usize {
        self.inner.config.max_total_memory
    }
}

#[cfg(test)]
mod tests {
    use crate::memory::{ResourceLimits, ResourceLimitsConfig};

    fn limits(total: usize) -> ResourceLimits {
        ResourceLimits::new(ResourceLimitsConfig {
            max_memory_per_call: 100,
            max_total_memory: total,
            max_concurrent: 4,
        })
    }

    #[test]
    fn resident_bytes_count_toward_the_total() {
        let l = limits(1_000);
        let _weights = l.reserve_resident(900).unwrap();
        assert_eq!(l.current_memory(), 900);
        assert!(
            l.try_acquire(100).is_ok(),
            "900 resident + 100 working fits"
        );
        assert!(
            l.try_acquire(101).is_err(),
            "per-call cap still applies to working bytes"
        );
    }

    #[test]
    fn working_reservation_fails_when_resident_fills_the_total() {
        let l = limits(1_000);
        let _weights = l.reserve_resident(950).unwrap();
        assert!(l.try_acquire(100).is_err(), "950 + 100 exceeds the total");
    }

    #[test]
    fn resident_reservation_larger_than_per_call_is_allowed() {
        let l = limits(10_000);
        assert!(
            l.reserve_resident(5_000).is_ok(),
            "weights are not per-call memory"
        );
    }

    #[test]
    fn resident_reservation_beyond_total_is_refused_and_not_leaked() {
        let l = limits(1_000);
        assert!(l.reserve_resident(1_001).is_err());
        assert_eq!(l.current_memory(), 0, "failed reservation must not leak");
    }

    #[test]
    fn dropping_the_guard_releases_resident_bytes() {
        let l = limits(1_000);
        let guard = l.reserve_resident(800).unwrap();
        assert!(l.check_resident_headroom(300).is_err());
        drop(guard);
        assert_eq!(l.current_memory(), 0);
        assert!(l.check_resident_headroom(300).is_ok());
    }
}
