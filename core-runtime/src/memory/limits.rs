//! Resource limit enforcement for CORE Runtime.
//!
//! Tracks and enforces memory and concurrency limits per inference call.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::engine::InferenceError;

#[path = "resident.rs"]
mod resident;
pub use resident::ResidentGuard;

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
            // Per-request working memory (weights are reserved at load).
            max_memory_per_call: 1024 * 1024 * 1024, // 1GB
            // Weights + working; also the Job cap. max(2 GiB, RAM/2).
            max_total_memory: super::host::default_total_memory(),
            max_concurrent: 2,
        }
    }
}

/// Shared state for resource tracking.
struct LimitsInner {
    config: ResourceLimitsConfig,
    current_memory: AtomicUsize,
    current_concurrent: AtomicUsize,
    /// Optional operator per-request ceiling (daemon env only; set once).
    per_call_ceiling: std::sync::OnceLock<usize>,
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
                per_call_ceiling: std::sync::OnceLock::new(),
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

        self.reserve_call(memory_bytes)
    }

    /// Reserve one request's working memory. Only an operator-set per-call
    /// ceiling ([`Self::set_per_call_ceiling`]) bounds a single request;
    /// otherwise the shared total does.
    pub fn try_acquire_working(
        &self,
        memory_bytes: usize,
    ) -> Result<ResourceGuard, InferenceError> {
        if let Some(&ceiling) = self.inner.per_call_ceiling.get() {
            if memory_bytes > ceiling {
                return Err(InferenceError::MemoryExceeded {
                    used: memory_bytes,
                    limit: ceiling,
                });
            }
        }
        self.reserve_call(memory_bytes)
    }

    /// Set the operator per-call ceiling once (daemon start, before serving).
    pub fn set_per_call_ceiling(&self, ceiling: usize) -> Result<(), usize> {
        self.inner.per_call_ceiling.set(ceiling)
    }

    /// The operator per-call ceiling, if one was set.
    pub fn per_call_ceiling(&self) -> Option<usize> {
        self.inner.per_call_ceiling.get().copied()
    }

    fn reserve_call(&self, memory_bytes: usize) -> Result<ResourceGuard, InferenceError> {
        let inner = &self.inner;
        // Reserve memory with a checked compare-and-swap: never wraps, and a
        // failed attempt never transiently inflates the shared counter.
        if let Err(used) = resident::try_reserve(
            &inner.current_memory,
            memory_bytes,
            inner.config.max_total_memory,
        ) {
            return Err(InferenceError::MemoryExceeded {
                used,
                limit: inner.config.max_total_memory,
            });
        }

        // Reserve a concurrency slot the same way; roll memory back on failure.
        if let Err(attempted) =
            resident::try_reserve(&inner.current_concurrent, 1, inner.config.max_concurrent)
        {
            inner
                .current_memory
                .fetch_sub(memory_bytes, Ordering::SeqCst);
            return Err(InferenceError::QueueFull {
                current: attempted,
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

    /// Maximum concurrent requests (`GG_CORE_MAX_CONCURRENT`); also the
    /// number of scheduler worker loops.
    pub fn max_concurrent(&self) -> usize {
        self.inner.config.max_concurrent
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
        self.inner
            .current_memory
            .fetch_sub(self.memory_bytes, Ordering::SeqCst);
        self.inner.current_concurrent.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
