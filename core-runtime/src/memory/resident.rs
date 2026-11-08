//! Resident (model-weight) memory accounting.
//!
//! Weights are charged once when a model is registered and released when it
//! is unloaded. They share the single `current_memory` counter with
//! per-request working reservations, so `resident + working <= total` holds
//! without a race between two counters. Resident reservations never take a
//! concurrency slot and are never compared against the per-call budget.
//!
//! Every reservation goes through [`try_reserve`]: a checked compare-and-swap
//! that cannot wrap `usize` and never publishes an over-limit value, so a
//! failed attempt cannot make a concurrent, otherwise-fitting reservation
//! fail, and no interleaving can admit more than the limit.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::{LimitsInner, ResourceLimits};
use crate::engine::InferenceError;

/// Atomically add `bytes` to `counter` iff the sum is `<= limit` and does not
/// overflow. On refusal returns the (saturated) total that was attempted.
pub(super) fn try_reserve(counter: &AtomicUsize, bytes: usize, limit: usize) -> Result<(), usize> {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            current.checked_add(bytes).filter(|total| *total <= limit)
        })
        .map(|_| ())
        .map_err(|current| current.saturating_add(bytes))
}

/// RAII reservation for a loaded (or loading) model's resident bytes.
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

impl ResidentGuard {
    /// Bytes currently held by this reservation.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Re-size the reservation in place (pre-load estimate -> backend-reported
    /// footprint). Shrinking always succeeds; growing is admitted against the
    /// total budget and, on refusal, leaves the reservation unchanged.
    pub fn resize(&mut self, new_bytes: usize) -> Result<(), InferenceError> {
        let total = self.inner.config.max_total_memory;
        if new_bytes > self.bytes {
            try_reserve(&self.inner.current_memory, new_bytes - self.bytes, total)
                .map_err(|used| InferenceError::MemoryExceeded { used, limit: total })?;
        } else {
            self.inner
                .current_memory
                .fetch_sub(self.bytes - new_bytes, Ordering::SeqCst);
        }
        self.bytes = new_bytes;
        Ok(())
    }
}

impl ResourceLimits {
    /// Reserve resident bytes for a model being loaded or registered. Fails
    /// with `MemoryExceeded` (limit = total budget) when it would not fit.
    pub fn reserve_resident(&self, bytes: usize) -> Result<ResidentGuard, InferenceError> {
        let limit = self.inner.config.max_total_memory;
        try_reserve(&self.inner.current_memory, bytes, limit)
            .map_err(|used| InferenceError::MemoryExceeded { used, limit })?;
        Ok(ResidentGuard {
            bytes,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Total-memory budget (resident + working).
    pub fn max_total_memory(&self) -> usize {
        self.inner.config.max_total_memory
    }
}

#[cfg(test)]
#[path = "resident_tests.rs"]
mod tests;
