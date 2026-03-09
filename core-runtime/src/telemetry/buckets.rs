// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Histogram buckets for Prometheus-compatible metrics export.
//!
//! Provides bucketed histograms with configurable boundaries for latency
//! and throughput measurements. Compatible with Prometheus histogram format.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Default latency buckets in milliseconds (Prometheus standard).
pub const DEFAULT_LATENCY_BUCKETS: [f64; 11] = [
    0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0,
];

/// Token throughput buckets (tokens per second).
pub const DEFAULT_THROUGHPUT_BUCKETS: [f64; 10] = [
    10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0,
];

/// Snapshot of a bucketed histogram for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketedHistogramSnapshot {
    pub boundaries: Vec<f64>,
    pub bucket_counts: Vec<u64>,
    pub count: u64,
    pub sum: f64,
}

/// Thread-safe bucketed histogram with configurable boundaries.
pub struct BucketedHistogram {
    boundaries: Vec<f64>,
    buckets: Vec<AtomicU64>,
    count: AtomicU64,
    sum: AtomicU64, // f64 bits stored as u64
}

impl BucketedHistogram {
    /// Create a new bucketed histogram with the given boundaries.
    pub fn new(boundaries: &[f64]) -> Self {
        let mut sorted = boundaries.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        Self {
            boundaries: sorted.clone(),
            buckets: (0..=sorted.len()).map(|_| AtomicU64::new(0)).collect(),
            count: AtomicU64::new(0),
            sum: AtomicU64::new(f64::to_bits(0.0)),
        }
    }

    /// Create histogram with default latency buckets.
    pub fn latency() -> Self {
        Self::new(&DEFAULT_LATENCY_BUCKETS)
    }

    /// Create histogram with default throughput buckets.
    pub fn throughput() -> Self {
        Self::new(&DEFAULT_THROUGHPUT_BUCKETS)
    }

    /// Record an observation.
    pub fn observe(&self, value: f64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.atomic_add_f64(value);

        let bucket_idx = self.find_bucket(value);
        self.buckets[bucket_idx].fetch_add(1, Ordering::Relaxed);
    }

    /// Get a snapshot of the histogram.
    pub fn snapshot(&self) -> BucketedHistogramSnapshot {
        BucketedHistogramSnapshot {
            boundaries: self.boundaries.clone(),
            bucket_counts: self
                .buckets
                .iter()
                .map(|b| b.load(Ordering::Relaxed))
                .collect(),
            count: self.count.load(Ordering::Relaxed),
            sum: f64::from_bits(self.sum.load(Ordering::Relaxed)),
        }
    }

    fn find_bucket(&self, value: f64) -> usize {
        for (i, &boundary) in self.boundaries.iter().enumerate() {
            if value <= boundary {
                return i;
            }
        }
        self.boundaries.len() // +Inf bucket
    }

    fn atomic_add_f64(&self, value: f64) {
        loop {
            let current = self.sum.load(Ordering::Relaxed);
            let new = f64::from_bits(current) + value;
            let result = self.sum.compare_exchange_weak(
                current,
                f64::to_bits(new),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            if result.is_ok() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_buckets() {
        let h = BucketedHistogram::new(&[1.0, 5.0, 10.0]);

        h.observe(0.5); // bucket 0 (<=1.0)
        h.observe(3.0); // bucket 1 (<=5.0)
        h.observe(7.0); // bucket 2 (<=10.0)
        h.observe(15.0); // bucket 3 (+Inf)

        let snap = h.snapshot();
        assert_eq!(snap.count, 4);
        assert_eq!(snap.bucket_counts, vec![1, 1, 1, 1]);
    }

    #[test]
    fn test_default_latency_buckets() {
        let h = BucketedHistogram::latency();
        assert_eq!(h.boundaries.len(), 11);
    }
}
