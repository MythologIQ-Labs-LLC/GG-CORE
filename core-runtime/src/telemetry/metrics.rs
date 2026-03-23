//! Metrics collection for CORE Runtime.
//!
//! Defines counters, gauges, and histograms for inference monitoring.
//! Uses the `metrics` facade - no network exporters included.

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};

/// Initialize metric descriptions.
///
/// Call once at startup to register metric metadata.
pub fn init_metrics() {
    // Request counters
    describe_counter!("core_requests_total", "Total inference requests");
    describe_counter!("core_requests_success", "Successful inference requests");
    describe_counter!("core_requests_failed", "Failed inference requests");

    // Latency histograms
    describe_histogram!("core_inference_latency_ms", "Inference latency in milliseconds");
    describe_histogram!(
        "core_tokenization_latency_ms",
        "Tokenization latency in milliseconds"
    );

    // Token counters
    describe_counter!("core_tokens_input_total", "Total input tokens processed");
    describe_counter!("core_tokens_output_total", "Total output tokens generated");

    // Resource gauges
    describe_gauge!("core_memory_pool_used_bytes", "Memory pool bytes in use");
    describe_gauge!("core_queue_depth", "Number of pending requests");
    describe_gauge!("core_active_sessions", "Number of active sessions");

    // Arena metrics (Tier 3)
    describe_gauge!("core_arena_used_bytes", "Arena allocator bytes in use");
    describe_counter!("core_arena_resets_total", "Arena reset count");

    // Speculative decoding (Tier 3)
    describe_counter!("core_speculative_drafts_total", "Total draft generation cycles");
    describe_counter!("core_speculative_accepted_tokens", "Draft tokens accepted");
    describe_counter!("core_speculative_rejected_tokens", "Draft tokens rejected");

    // Admission control
    describe_counter!(
        "core_admission_rejections_total",
        "Requests rejected before inference due to resource limits"
    );

    // Model pool warm-switch latency
    describe_histogram!(
        "core_model_switch_latency_seconds",
        "Model pool warm-switch latency in seconds"
    );
}

/// Record model pool warm-switch latency.
pub fn record_model_switch_latency(model_id: &str, latency_secs: f64) {
    histogram!(
        "core_model_switch_latency_seconds",
        "model" => model_id.to_string()
    )
    .record(latency_secs);
}

/// Record a successful inference request.
pub fn record_request_success(model: &str, latency_ms: u64, tokens_out: u64) {
    counter!("core_requests_total", "model" => model.to_string()).increment(1);
    counter!("core_requests_success", "model" => model.to_string()).increment(1);
    counter!("core_tokens_output_total", "model" => model.to_string()).increment(tokens_out);
    histogram!("core_inference_latency_ms", "model" => model.to_string()).record(latency_ms as f64);
}

/// Record a failed inference request.
pub fn record_request_failure(model: &str, error_type: &str) {
    counter!("core_requests_total", "model" => model.to_string()).increment(1);
    counter!(
        "core_requests_failed",
        "model" => model.to_string(),
        "error" => error_type.to_string()
    )
    .increment(1);
}

/// Record memory pool usage.
pub fn record_memory_pool(used_bytes: usize) {
    gauge!("core_memory_pool_used_bytes").set(used_bytes as f64);
}

/// Record queue depth.
pub fn record_queue_depth(depth: usize) {
    gauge!("core_queue_depth").set(depth as f64);
}

/// Record an admission rejection (resource limits exceeded before inference starts).
///
/// This is distinct from `record_request_failure`, which tracks execution failures.
pub fn record_admission_rejection(model: &str, reason: &str) {
    counter!("core_requests_total", "model" => model.to_string()).increment(1);
    counter!(
        "core_admission_rejections_total",
        "model" => model.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

/// Record speculative decoding cycle stats.
pub fn record_speculative_cycle(accepted: usize, rejected: usize) {
    counter!("core_speculative_drafts_total").increment(1);
    counter!("core_speculative_accepted_tokens").increment(accepted as u64);
    counter!("core_speculative_rejected_tokens").increment(rejected as u64);
}
