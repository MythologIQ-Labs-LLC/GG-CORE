//! Memory pooling for efficient buffer reuse.
//!
//! Uses parking_lot::Mutex for fast synchronous locking.
//! No async overhead or tokio runtime requirement.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Configuration for memory pool.
#[derive(Debug, Clone)]
pub struct MemoryPoolConfig {
    pub buffer_size: usize,
    pub max_buffers: usize,
}

impl Default for MemoryPoolConfig {
    fn default() -> Self {
        Self {
            buffer_size: 4096,
            max_buffers: 64,
        }
    }
}

/// A buffer obtained from the memory pool.
pub struct PooledBuffer {
    data: Vec<u8>,
    pool: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl PooledBuffer {
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let data = std::mem::take(&mut self.data);
        // Synchronous return to pool - no tokio::spawn needed
        self.pool.lock().push_back(data);
    }
}

/// Thread-safe memory pool for buffer reuse.
/// Uses synchronous locking for minimal overhead.
pub struct MemoryPool {
    buffers: Arc<Mutex<VecDeque<Vec<u8>>>>,
    config: MemoryPoolConfig,
}

impl MemoryPool {
    pub fn new(config: MemoryPoolConfig) -> Self {
        Self {
            buffers: Arc::new(Mutex::new(VecDeque::with_capacity(config.max_buffers))),
            config,
        }
    }

    /// Acquire a buffer from the pool, or allocate a new one.
    /// Synchronous - no async overhead.
    pub fn acquire(&self) -> PooledBuffer {
        let data = self
            .buffers
            .lock()
            .pop_front()
            .unwrap_or_else(|| vec![0u8; self.config.buffer_size]);

        PooledBuffer {
            data,
            pool: self.buffers.clone(),
        }
    }

    /// Async version for compatibility with async code paths.
    pub async fn acquire_async(&self) -> PooledBuffer {
        self.acquire()
    }

    /// Current number of available buffers in pool.
    pub fn available(&self) -> usize {
        self.buffers.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pool_acquire_and_release() {
        let config = MemoryPoolConfig {
            buffer_size: 1024,
            max_buffers: 2,
        };
        let pool = MemoryPool::new(config);

        // Initially empty
        assert_eq!(pool.available(), 0);

        {
            // Acquire allocates a new buffer
            let buffer = pool.acquire();
            assert_eq!(buffer.len(), 1024);
            assert_eq!(pool.available(), 0);
        } // buffer dropped here, returned to pool

        // Pool now has 1 available buffer
        assert_eq!(pool.available(), 1);

        {
            // Acquire reuses the buffer
            let buffer2 = pool.acquire();
            assert_eq!(buffer2.len(), 1024);
            // Pool is empty again
            assert_eq!(pool.available(), 0);
        }

        // Buffer returned again
        assert_eq!(pool.available(), 1);
    }
}
