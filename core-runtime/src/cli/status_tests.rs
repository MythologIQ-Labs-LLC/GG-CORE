//! Tests for the status command.

use super::super::status_format::*;
use super::*;

#[test]
fn test_format_uptime() {
    assert_eq!(format_uptime(0), "0m");
    assert_eq!(format_uptime(59), "0m");
    assert_eq!(format_uptime(60), "1m");
    assert_eq!(format_uptime(3600), "1h 0m");
    assert_eq!(format_uptime(3661), "1h 1m");
    assert_eq!(format_uptime(86400), "1d 0h 0m");
    assert_eq!(format_uptime(90061), "1d 1h 1m");
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1048576), "1.0 MB");
    assert_eq!(format_bytes(1073741824), "1.0 GB");
}

#[test]
fn test_truncate() {
    assert_eq!(truncate("short", 10), "short");
    assert_eq!(truncate("this is a very long string", 10), "this is...");
}

#[test]
fn test_health_state_display() {
    assert_eq!(format!("{}", HealthState::Healthy), "healthy");
    assert_eq!(format!("{}", HealthState::Degraded), "degraded");
    assert_eq!(format!("{}", HealthState::Unhealthy), "unhealthy");
}

#[test]
fn test_model_state_display() {
    assert_eq!(format!("{}", ModelState::Loading), "loading");
    assert_eq!(format!("{}", ModelState::Ready), "ready");
    assert_eq!(format!("{}", ModelState::Unloading), "unloading");
    assert_eq!(format!("{}", ModelState::Error), "error");
}

#[test]
fn test_system_status_serialization() {
    let status = SystemStatus {
        health: HealthState::Healthy,
        uptime_secs: 3600,
        version: VersionInfo {
            version: "0.6.5".to_string(),
            commit: "abc123".to_string(),
            build_date: "2026-02-18".to_string(),
            rust_version: "1.75.0".to_string(),
        },
        models: vec![],
        requests: RequestStats {
            total_requests: 1000,
            successful_requests: 990,
            failed_requests: 10,
            requests_per_second: 10.5,
            avg_latency_ms: 50.0,
            p50_latency_ms: Some(45.0),
            p95_latency_ms: Some(100.0),
            p99_latency_ms: Some(150.0),
            tokens_generated: 50000,
            tokens_per_second: 25.0,
        },
        resources: ResourceUtilization {
            memory_rss_bytes: 4 * 1024 * 1024 * 1024,
            kv_cache_bytes: 2 * 1024 * 1024 * 1024,
            arena_bytes: 512 * 1024 * 1024,
            memory_limit_bytes: 8 * 1024 * 1024 * 1024,
            memory_utilization_percent: 50.0,
            cpu_utilization_percent: 75.0,
            active_threads: 8,
        },
        scheduler: SchedulerStatus {
            queue_depth: 5,
            active_batches: 2,
            pending_requests: 10,
            completed_requests: 1000,
            avg_batch_size: 4.5,
        },
        gpus: None,
        recent_events: vec![],
        #[cfg(feature = "advanced")]
        speculative_stats: None,
    };

    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"health\":\"healthy\""));
    assert!(json.contains("\"uptime_secs\":3600"));
}

// B-21h: `status` surfaces live speculative stats derived from the Prometheus
// counters that flow over the metrics channel.
#[cfg(feature = "advanced")]
#[test]
fn build_status_populates_speculative_stats_from_metrics() {
    use crate::telemetry::MetricsSnapshot;
    use std::collections::HashMap;

    let snap = |drafts: u64, accepted: u64, rejected: u64| {
        let mut counters = HashMap::new();
        if drafts > 0 {
            counters.insert("core_speculative_drafts_total".to_string(), drafts);
            counters.insert("core_speculative_accepted_tokens".to_string(), accepted);
            counters.insert("core_speculative_rejected_tokens".to_string(), rejected);
        }
        MetricsSnapshot {
            counters,
            gauges: HashMap::new(),
            histograms: HashMap::new(),
            bucketed_histograms: HashMap::new(),
        }
    };

    // Active speculation: counts + acceptance_rate derived from the counters.
    let stats = build_speculative_stats(&Some(snap(10, 30, 10))).expect("Some when drafts > 0");
    assert_eq!(stats.verification_steps, 10);
    assert_eq!(stats.accepted_tokens, 30);
    assert_eq!(stats.rejected_tokens, 10);
    assert_eq!(stats.draft_tokens_generated, 40);
    assert!((stats.acceptance_rate - 0.75).abs() < 1e-6);
    assert!((stats.mean_accepted_length - 3.0).abs() < 1e-6);

    // No speculative activity, or no metrics at all -> None.
    assert!(build_speculative_stats(&Some(snap(0, 0, 0))).is_none());
    assert!(build_speculative_stats(&None).is_none());
}

#[test]
fn status_never_invents_quantiles_from_min_max_snapshot() {
    use crate::telemetry::{HistogramSummary, MetricsSnapshot};
    use std::collections::HashMap;

    let mut histograms = HashMap::new();
    histograms.insert(
        "core_inference_latency_ms".to_string(),
        HistogramSummary {
            count: 10,
            sum: 1_000.0,
            min: 1.0,
            max: 500.0,
        },
    );
    let metrics = MetricsSnapshot {
        counters: HashMap::new(),
        gauges: HashMap::new(),
        histograms,
        bucketed_histograms: HashMap::new(),
    };
    let status = build_status(true, None, Some(metrics), None);
    assert_eq!(status.requests.avg_latency_ms, 100.0);
    assert_eq!(status.requests.p50_latency_ms, None);
    assert_eq!(status.requests.p95_latency_ms, None);
    assert_eq!(status.requests.p99_latency_ms, None);

    let serialized = serde_json::to_value(&status).unwrap();
    assert!(serialized["requests"]["p50_latency_ms"].is_null());
    assert!(serialized["requests"]["p95_latency_ms"].is_null());
    assert!(serialized["requests"]["p99_latency_ms"].is_null());
}

#[test]
fn missing_quantiles_are_displayed_as_unavailable() {
    assert_eq!(display_quantile(None), "N/A");
    assert_eq!(display_quantile(Some(f64::NAN)), "N/A");
    assert_eq!(display_quantile(Some(f64::INFINITY)), "N/A");
    assert_eq!(display_quantile(Some(-1.0)), "N/A");
    assert_eq!(display_quantile(Some(42.5)), "42.5ms");
}

#[test]
fn cli_model_status_preserves_daemon_average_without_dividing_twice() {
    use crate::ipc::protocol::{ModelInfo, ModelsListResponse};

    let make_model = |name: &str, state: &str| ModelInfo {
        handle_id: 42,
        name: name.to_string(),
        format: "gguf".to_string(),
        size_bytes: 100,
        memory_bytes: 200,
        state: state.to_string(),
        request_count: 10,
        avg_latency_ms: 40.0,
        loaded_at: "2026-11-18T00:00:00Z".to_string(),
        capabilities: Vec::new(),
    };
    let response = ModelsListResponse {
        models: vec![make_model("ready", "ready"), make_model("bad", "new-state")],
        total_memory_bytes: 400,
    };
    let models = build_models_list(&Some(response));
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].avg_latency_ms, 40.0);
    assert_eq!(models[0].state, ModelState::Ready);
    assert_eq!(models[1].avg_latency_ms, 40.0);
    assert_eq!(models[1].state, ModelState::Error);
}

#[test]
fn cli_status_truncation_handles_multibyte_model_names_and_events() {
    let label = "模型🚀one";
    assert_eq!(truncate(label, 0), "");
    assert_eq!(truncate(label, 2), "..");
    assert_eq!(truncate(label, 3), "...");
    assert_eq!(truncate(label, 6), "模...");
    assert_eq!(truncate(label, 7), "模...");
    assert_eq!(truncate(label, 9), "模型...");
    assert_eq!(truncate(label, 13), label);
    assert!(truncate(&label.repeat(10), 27).len() <= 27);
}
