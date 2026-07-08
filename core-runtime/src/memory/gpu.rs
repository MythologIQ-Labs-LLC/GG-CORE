//! GPU memory tracking and management.

use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GpuMemoryError {
    #[error("GPU memory exhausted: requested {requested} bytes, available {available} bytes")]
    OutOfMemory { requested: usize, available: usize },
}

/// Configuration for GPU memory management.
#[derive(Debug, Clone)]
pub struct GpuMemoryConfig {
    pub max_bytes: usize,
}

impl Default for GpuMemoryConfig {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024 * 1024, // 4 GB default
        }
    }
}

/// Tracks GPU memory allocation.
pub struct GpuMemory {
    allocated: AtomicUsize,
    config: GpuMemoryConfig,
}

impl GpuMemory {
    pub fn new(config: GpuMemoryConfig) -> Self {
        Self {
            allocated: AtomicUsize::new(0),
            config,
        }
    }

    /// Reserve GPU memory. Returns error if insufficient.
    pub fn reserve(&self, bytes: usize) -> Result<GpuReservation, GpuMemoryError> {
        let current = self.allocated.fetch_add(bytes, Ordering::SeqCst);
        let new_total = current + bytes;

        if new_total > self.config.max_bytes {
            self.allocated.fetch_sub(bytes, Ordering::SeqCst);
            return Err(GpuMemoryError::OutOfMemory {
                requested: bytes,
                available: self.config.max_bytes.saturating_sub(current),
            });
        }

        Ok(GpuReservation { bytes })
    }

    /// Release previously reserved memory.
    pub fn release(&self, reservation: GpuReservation) {
        self.allocated
            .fetch_sub(reservation.bytes, Ordering::SeqCst);
    }

    pub fn allocated(&self) -> usize {
        self.allocated.load(Ordering::SeqCst)
    }

    pub fn available(&self) -> usize {
        self.config.max_bytes.saturating_sub(self.allocated())
    }
}

/// Handle representing reserved GPU memory.
pub struct GpuReservation {
    bytes: usize,
}

impl GpuReservation {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
