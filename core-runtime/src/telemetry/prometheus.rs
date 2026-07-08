// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Prometheus text format exporter.
//!
//! Exports metrics in Prometheus exposition format for scraping.
//! Format spec: https://prometheus.io/docs/instrumenting/exposition_formats/

use std::fmt::Write;

use super::buckets::BucketedHistogramSnapshot;
use super::store::MetricsSnapshot;

/// Metric descriptions for HELP lines.
pub struct MetricHelp {
    pub name: &'static str,
    pub help: &'static str,
    pub metric_type: &'static str,
}

/// Standard metric definitions for GG-CORE.
pub const METRIC_DEFINITIONS: &[MetricHelp] = &[
    MetricHelp {
        name: "core_requests_total",
        help: "Total inference requests",
        metric_type: "counter",
    },
    MetricHelp {
        name: "core_requests_success",
        help: "Successful inference requests",
        metric_type: "counter",
    },
    MetricHelp {
        name: "core_requests_failed",
        help: "Failed inference requests",
        metric_type: "counter",
    },
    MetricHelp {
        name: "core_tokens_generated",
        help: "Total tokens generated",
        metric_type: "counter",
    },
    MetricHelp {
        name: "core_queue_depth",
        help: "Current request queue depth",
        metric_type: "gauge",
    },
    MetricHelp {
        name: "core_memory_used_bytes",
        help: "Memory currently in use",
        metric_type: "gauge",
    },
    MetricHelp {
        name: "core_models_loaded",
        help: "Number of loaded models",
        metric_type: "gauge",
    },
    MetricHelp {
        name: "core_latency_ms",
        help: "Request latency in milliseconds",
        metric_type: "histogram",
    },
    MetricHelp {
        name: "core_throughput_tps",
        help: "Token throughput per second",
        metric_type: "histogram",
    },
];

/// Encode metrics snapshot to Prometheus text format.
pub fn encode_prometheus(snapshot: &MetricsSnapshot) -> String {
    let mut output = String::with_capacity(4096);

    // Counters
    for (name, value) in &snapshot.counters {
        write_metric_header(&mut output, name);
        writeln!(output, "{name} {value}").unwrap();
    }

    // Gauges
    for (name, value) in &snapshot.gauges {
        write_metric_header(&mut output, name);
        writeln!(output, "{name} {value}").unwrap();
    }

    // Summary histograms (basic stats)
    for (name, summary) in &snapshot.histograms {
        write_metric_header(&mut output, name);
        writeln!(output, "{name}_count {}", summary.count).unwrap();
        writeln!(output, "{name}_sum {}", summary.sum).unwrap();
    }

    output
}

/// Encode bucketed histogram to Prometheus format.
pub fn encode_bucketed_histogram(name: &str, snap: &BucketedHistogramSnapshot) -> String {
    let mut output = String::with_capacity(512);

    write_metric_header(&mut output, name);

    // Cumulative bucket counts (Prometheus requirement)
    let mut cumulative = 0u64;
    for (i, &boundary) in snap.boundaries.iter().enumerate() {
        cumulative += snap.bucket_counts[i];
        writeln!(output, "{name}_bucket{{le=\"{boundary}\"}} {cumulative}").unwrap();
    }

    // +Inf bucket
    cumulative += snap.bucket_counts.last().copied().unwrap_or(0);
    writeln!(output, "{name}_bucket{{le=\"+Inf\"}} {cumulative}").unwrap();

    // Count and sum
    writeln!(output, "{name}_count {}", snap.count).unwrap();
    writeln!(output, "{name}_sum {}", snap.sum).unwrap();

    output
}

fn write_metric_header(output: &mut String, name: &str) {
    if let Some(def) = METRIC_DEFINITIONS.iter().find(|d| d.name == name) {
        writeln!(output, "# HELP {name} {}", def.help).unwrap();
        writeln!(output, "# TYPE {name} {}", def.metric_type).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_encode_counter() {
        let mut snapshot = MetricsSnapshot {
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
            bucketed_histograms: HashMap::new(),
        };
        snapshot
            .counters
            .insert("core_requests_total".to_string(), 42);

        let output = encode_prometheus(&snapshot);
        assert!(output.contains("# HELP core_requests_total"));
        assert!(output.contains("# TYPE core_requests_total counter"));
        assert!(output.contains("core_requests_total 42"));
    }

    #[test]
    fn test_encode_bucketed_histogram() {
        let snap = BucketedHistogramSnapshot {
            boundaries: vec![1.0, 5.0, 10.0],
            bucket_counts: vec![2, 3, 1, 1],
            count: 7,
            sum: 25.5,
        };

        let output = encode_bucketed_histogram("test_latency", &snap);
        assert!(output.contains("test_latency_bucket{le=\"1\"} 2"));
        assert!(output.contains("test_latency_bucket{le=\"5\"} 5"));
        assert!(output.contains("test_latency_bucket{le=\"+Inf\"} 7"));
    }
}
