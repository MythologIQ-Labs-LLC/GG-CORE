//! Telemetry module for CORE Runtime.
//!
//! Provides structured logging, distributed tracing, and metrics collection.
//! All output is file-based or via existing IPC - no network dependencies.

pub mod buckets;
mod logging;
mod metrics;
pub mod prometheus;
pub mod security_log;
pub mod span_export;
mod spans;
mod store;

pub use buckets::{BucketedHistogram, BucketedHistogramSnapshot};
pub use logging::{init_logging, LogConfig, LogError, LogFormat};
pub use metrics::{
    init_metrics, record_admission_rejection, record_memory_pool, record_model_switch_latency,
    record_queue_depth, record_request_failure, record_request_success, record_speculative_cycle,
};
pub use prometheus::{encode_bucketed_histogram, encode_prometheus};
pub use security_log::{log_security_event, SecurityEvent, SecuritySeverity};
pub use span_export::{ExportableSpan, SpanAttributeValue, SpanCollector, SpanStatus};
pub use spans::{RequestSpan, SpanExt};
pub use store::{HistogramSummary, MetricsSnapshot, MetricsStore};
