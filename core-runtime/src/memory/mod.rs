//! Memory management module for CORE Runtime.
//!
//! Provides pooled memory allocation, GPU memory tracking, context caching,
//! arena allocation, paged KV-cache, and resource limit enforcement.

mod arena;
mod cache;
mod gpu;
pub mod kv_cache;
pub mod kv_cache_config;
pub mod kv_cache_core;
mod kv_cache_ops;
pub mod kv_quant;
mod limits;
pub mod paged;
mod pool;
pub mod prompt_cache;

pub use arena::{Arena, ArenaPool, ArenaSlice};
pub use cache::{ContextCache, ContextCacheConfig, KvCache, KvCacheEntry};
pub use gpu::{GpuMemory, GpuMemoryConfig, GpuMemoryError};
pub use kv_cache::{
    EvictionPolicy, KvCacheConfig, KvCacheError, KvCacheManager, KvCacheStats, SequenceId,
    SlidingWindowConfig,
};
pub use kv_quant::{compute_scale, dequantize, quantize_to, Q8KvStore};
pub use limits::{ResourceGuard, ResourceLimits, ResourceLimitsConfig};
pub use paged::{Page, PageId, PageTable, PAGE_TOKENS};
pub use pool::{MemoryPool, MemoryPoolConfig, PooledBuffer};
pub use prompt_cache::{CachedKv, PromptCache};
