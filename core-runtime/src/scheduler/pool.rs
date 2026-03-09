//! Thread pool configuration for inference workers.

use std::num::NonZeroUsize;

/// Thread pool configuration for inference workers.
#[derive(Debug, Clone)]
pub struct ThreadPoolConfig {
    /// Number of worker threads.
    pub worker_threads: NonZeroUsize,
    /// Stack size per thread in bytes.
    pub stack_size: usize,
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        let threads = std::thread::available_parallelism().unwrap_or(NonZeroUsize::new(4).unwrap());
        Self {
            worker_threads: threads,
            stack_size: 2 * 1024 * 1024, // 2MB
        }
    }
}

impl ThreadPoolConfig {
    /// Create config with specific thread count.
    pub fn with_threads(count: usize) -> Self {
        Self {
            worker_threads: NonZeroUsize::new(count.max(1)).unwrap(),
            ..Default::default()
        }
    }

    /// Create config optimized for inference (fewer threads, larger stacks).
    pub fn for_inference() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            worker_threads: NonZeroUsize::new((cores / 2).max(2)).unwrap(),
            stack_size: 4 * 1024 * 1024, // 4MB for model weights
        }
    }
}
