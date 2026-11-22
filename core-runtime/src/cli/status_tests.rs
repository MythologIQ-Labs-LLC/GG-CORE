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

// Quantiles must not be synthesized from a min/max-only IPC histogram.
#[test]
fn test_missing_quantiles_display_as_unavailable() {
    assert_eq!(display_quantile(None), "N/A");
    assert_eq!(display_quantile(Some(0.0)), "0.0ms");
    assert_eq!(display_quantile(Some(f64::NAN)), "N/A");
    assert_eq!(display_quantile(Some(f64::INFINITY)), "N/A");
    assert_eq!(display_quantile(Some(-1.0)), "N/A");
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

#[cfg(feature = "advanced")]
#[test]
fn speculative_status_does_not_fabricate_ipc_latency_or_speedup() {
    let stats = SpeculativeSessionStats {
        draft_tokens_generated: 40,
        verification_steps: 10,
        accepted_tokens: 30,
        rejected_tokens: 10,
        mean_accepted_length: 3.0,
        acceptance_rate: 0.75,
        // These internal values are placeholders in the CLI's IPC projection,
        // not observed timings or measured performance.
        mean_draft_latency_us: 0.0,
        mean_verify_latency_us: 0.0,
        net_speedup: 0.0,
        auto_disable_count: 0,
        auto_disable_reason: None,
    };
    let rendered = format_speculative_stats(&stats);
    assert!(rendered.contains("75.0% acceptance"));
    assert!(rendered.contains("Steps: 10"));
    assert!(rendered.contains("Draft tokens: 40"));
    assert!(rendered.contains("speedup N/A (IPC)"));
    assert!(rendered.contains("Mean draft latency: N/A (IPC)"));
    assert!(rendered.contains("Mean verify latency: N/A (IPC)"));
    assert!(!rendered.contains("0.00x"));
    assert!(!rendered.contains("0.0 us"));
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
fn status_from_partial_ipc_is_degraded_not_healthy() {
    use crate::health::{HealthReport, HealthState as RuntimeHealthState};
    use crate::ipc::protocol::ModelsListResponse;
    use crate::telemetry::MetricsSnapshot;
    use std::collections::HashMap;

    let snapshot = MetricsSnapshot {
        counters: HashMap::new(),
        gauges: HashMap::new(),
        histograms: HashMap::new(),
        bucketed_histograms: HashMap::new(),
    };
    let empty_models = ModelsListResponse {
        models: Vec::new(),
        total_memory_bytes: 0,
    };
    let healthy_report = HealthReport {
        state: RuntimeHealthState::Healthy,
        ready: true,
        accepting_requests: true,
        models_loaded: 0,
        memory_used_bytes: 0,
        queue_depth: 0,
        uptime_secs: 10,
    };

    // A healthy daemon with successful empty responses is healthy.
    let complete = build_status(
        true,
        Some(healthy_report.clone()),
        Some(snapshot.clone()),
        Some(empty_models.clone()),
    );
    assert_eq!(complete.health, HealthState::Healthy);
    assert!(complete.recent_events.is_empty());

    // Failed IPC reads previously became plausible empty/zero values while
    // health stayed Healthy. Do not misrepresent missing evidence as zero use.
    let missing_metrics = build_status(
        true,
        Some(healthy_report.clone()),
        None,
        Some(empty_models.clone()),
    );
    assert_eq!(missing_metrics.health, HealthState::Degraded);
    assert_eq!(missing_metrics.recent_events.len(), 1);
    assert!(missing_metrics.recent_events[0]
        .message
        .contains("Metrics IPC"));

    let missing_models = build_status(
        true,
        Some(healthy_report.clone()),
        Some(snapshot.clone()),
        None,
    );
    assert_eq!(missing_models.health, HealthState::Degraded);
    assert_eq!(missing_models.recent_events.len(), 1);
    assert!(missing_models.recent_events[0]
        .message
        .contains("Models IPC"));

    let both_missing = build_status(true, Some(healthy_report.clone()), None, None);
    assert_eq!(both_missing.health, HealthState::Degraded);
    assert_eq!(both_missing.recent_events.len(), 2);
    let serialized = serde_json::to_value(&both_missing).unwrap();
    assert_eq!(serialized["health"], "degraded");
    // Failed IPC must not masquerade as a complete healthy report.
    // The percentile representation is verified by its separate contract test.
    assert_eq!(serialized["recent_events"][0]["severity"], "warning");

    // A negative health report is not overridden by successfully fetched
    // metrics/models, nor does degraded partial telemetry hide it.
    let mut unhealthy_report = healthy_report.clone();
    unhealthy_report.state = RuntimeHealthState::Unhealthy;
    unhealthy_report.ready = false;
    unhealthy_report.accepting_requests = false;
    let unhealthy = build_status(
        false,
        Some(unhealthy_report),
        Some(snapshot),
        Some(empty_models),
    );
    assert_eq!(unhealthy.health, HealthState::Unhealthy);
    assert!(unhealthy.recent_events.is_empty());

    // Full health responses use `ok=ready`. A queue-saturation or
    // require-model condition is degraded, not actually unhealthy.
    let degraded_report = HealthReport {
        state: RuntimeHealthState::Degraded,
        ready: false,
        accepting_requests: true,
        ..healthy_report.clone()
    };
    let degraded = build_status(
        false,
        Some(degraded_report),
        Some(MetricsSnapshot {
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
            bucketed_histograms: HashMap::new(),
        }),
        Some(ModelsListResponse {
            models: Vec::new(),
            total_memory_bytes: 0,
        }),
    );
    assert_eq!(degraded.health, HealthState::Degraded);

    // A full health check returning no full report is incomplete even if
    // the daemon replied with ok=true and other IPC reads succeeded.
    let no_report = build_status(true, None, None, None);
    assert_eq!(no_report.health, HealthState::Degraded);
    assert_eq!(no_report.recent_events.len(), 3);
    assert!(no_report.recent_events[0].message.contains("Full health"));

    // An unhealthy daemon always takes precedence over missing diagnostics.
    let unhealthy_no_report = build_status(false, None, None, None);
    assert_eq!(unhealthy_no_report.health, HealthState::Unhealthy);
}
