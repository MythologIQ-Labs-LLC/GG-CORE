//! Request scheduling module for CORE Runtime.
//!
//! Manages request queuing, prioritization, batching, continuous batching,
//! deduplication, and thread pool configuration.

mod batch;
pub mod continuous;
mod dedup;
mod pool;
mod priority;
mod queue;
mod queued_request;
#[cfg(test)]
mod queue_tests;
pub mod streaming_queue;
pub mod thread_pool;
pub mod thread_pool_types;
pub mod worker;
pub(crate) mod worker_streaming;

pub use batch::{BatchConfig, BatchProcessor, RequestBatch};
pub use continuous::{
    BatchSlot, ContinuousBatcher, PendingRequest, RequestId, RequestPhase, StepResult,
};
pub use dedup::{CachedOutput, DedupResult, OutputCache, OutputCacheConfig};
pub use pool::ThreadPoolConfig;
pub use priority::{Priority, PriorityQueue};
pub use queue::{QueuedRequest, RequestQueue, RequestQueueConfig, ResponseRx};
pub use streaming_queue::StreamingQueuedRequest;
pub use worker::{spawn_worker, spawn_worker_with_registry};
pub use thread_pool::{
    TaskPriority, ThreadPool, ThreadPoolConfig as TunableThreadPoolConfig, ThreadPoolStats,
};
