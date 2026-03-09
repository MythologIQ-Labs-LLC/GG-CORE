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
            max_memory_per_call: 1024 * 1024 * 1024,  // 1GB
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
        let prev_memory = inner
            .current_memory
            .fetch_add(memory_bytes, Ordering::SeqCst);
        if prev_memory + memory_bytes > inner.config.max_total_memory {
            inner
                .current_memory
                .fetch_sub(memory_bytes, Ordering::SeqCst);
            return Err(InferenceError::MemoryExceeded {
                used: prev_memory + memory_bytes,
                limit: inner.config.max_total_memory,
            });
        }

        // Try to reserve concurrency slot
        let prev_concurrent = inner.current_concurrent.fetch_add(1, Ordering::SeqCst);
        if prev_concurrent >= inner.config.max_concurrent {
            inner.current_concurrent.fetch_sub(1, Ordering::SeqCst);
            inner
                .current_memory
                .fetch_sub(memory_bytes, Ordering::SeqCst);
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
}

/// RAII guard that releases resources when dropped.
pub struct ResourceGuard {
    memory_bytes: usize,
    inner: Arc<LimitsInner>,
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.inner
            .current_memory
            .fetch_sub(self.memory_bytes, Ordering::SeqCst);
        self.inner.current_concurrent.fetch_sub(1, Ordering::SeqCst);
    }
}
