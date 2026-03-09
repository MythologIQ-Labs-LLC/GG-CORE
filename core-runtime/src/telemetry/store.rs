//! Thread-safe metrics storage for IPC export.
//!
//! Provides a composable, value-oriented store that complements the `metrics`
//! crate facade. Values are stored here for IPC export while the facade handles
//! tracing integration.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::buckets::{BucketedHistogram, BucketedHistogramSnapshot};

/// Snapshot of all metrics at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub counters: HashMap<String, u64>,
    pub gauges: HashMap<String, f64>,
    pub histograms: HashMap<String, HistogramSummary>,
    #[serde(default)]
    pub bucketed_histograms: HashMap<String, BucketedHistogramSnapshot>,
}

/// Summary statistics for a histogram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramSummary {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
}

/// Internal histogram data with atomic fields.
struct HistogramData {
    count: AtomicU64,
    sum: AtomicU64, // f64 bits stored as u64
    min: AtomicU64, // f64 bits stored as u64
    max: AtomicU64, // f64 bits stored as u64
}

impl HistogramData {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sum: AtomicU64::new(f64::to_bits(0.0)),
            min: AtomicU64::new(f64::to_bits(f64::MAX)),
            max: AtomicU64::new(f64::to_bits(f64::MIN)),
        }
    }

    fn record(&self, value: f64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.atomic_add_f64(&self.sum, value);
        self.atomic_min_f64(&self.min, value);
        self.atomic_max_f64(&self.max, value);
    }

    fn atomic_add_f64(&self, atomic: &AtomicU64, value: f64) {
        loop {
            let current = atomic.load(Ordering::Relaxed);
            let new = f64::from_bits(current) + value;
            if atomic
                .compare_exchange_weak(
                    current,
                    f64::to_bits(new),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn atomic_min_f64(&self, atomic: &AtomicU64, value: f64) {
        loop {
            let current = atomic.load(Ordering::Relaxed);
            let current_f64 = f64::from_bits(current);
            if value >= current_f64 {
                break;
            }
            if atomic
                .compare_exchange_weak(
                    current,
                    f64::to_bits(value),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn atomic_max_f64(&self, atomic: &AtomicU64, value: f64) {
        loop {
            let current = atomic.load(Ordering::Relaxed);
            let current_f64 = f64::from_bits(current);
            if value <= current_f64 {
                break;
            }
            if atomic
                .compare_exchange_weak(
                    current,
                    f64::to_bits(value),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn to_summary(&self) -> HistogramSummary {
        let count = self.count.load(Ordering::Relaxed);
        let sum = f64::from_bits(self.sum.load(Ordering::Relaxed));
        let min = f64::from_bits(self.min.load(Ordering::Relaxed));
        let max = f64::from_bits(self.max.load(Ordering::Relaxed));

        HistogramSummary {
            count,
            sum,
            min: if count == 0 { 0.0 } else { min },
            max: if count == 0 { 0.0 } else { max },
        }
    }
}

/// Thread-safe metrics store for IPC export.
pub struct MetricsStore {
    counters: RwLock<HashMap<String, AtomicU64>>,
    gauges: RwLock<HashMap<String, AtomicU64>>,
    histograms: RwLock<HashMap<String, HistogramData>>,
    bucketed_histograms: RwLock<HashMap<String, BucketedHistogram>>,
}

impl MetricsStore {
    /// Create a new empty metrics store.
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            gauges: RwLock::new(HashMap::new()),
            histograms: RwLock::new(HashMap::new()),
            bucketed_histograms: RwLock::new(HashMap::new()),
        }
    }

    /// Increment a counter by the given value.
    pub fn increment_counter(&self, name: &str, value: u64) {
        let counters = self.counters.read().unwrap();
        if let Some(counter) = counters.get(name) {
            counter.fetch_add(value, Ordering::Relaxed);
            return;
        }
        drop(counters);

        let mut counters = self.counters.write().unwrap();
        counters
            .entry(name.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(value, Ordering::Relaxed);
    }

    /// Set a gauge to the given value.
    pub fn set_gauge(&self, name: &str, value: f64) {
        let gauges = self.gauges.read().unwrap();
        if let Some(gauge) = gauges.get(name) {
            gauge.store(f64::to_bits(value), Ordering::Relaxed);
            return;
        }
        drop(gauges);

        let mut gauges = self.gauges.write().unwrap();
        gauges
            .entry(name.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .store(f64::to_bits(value), Ordering::Relaxed);
    }

    /// Record a histogram observation.
    pub fn record_histogram(&self, name: &str, value: f64) {
        let histograms = self.histograms.read().unwrap();
        if let Some(histogram) = histograms.get(name) {
            histogram.record(value);
            return;
        }
        drop(histograms);

        let mut histograms = self.histograms.write().unwrap();
        let histogram = histograms
            .entry(name.to_string())
            .or_insert_with(HistogramData::new);
        histogram.record(value);
    }

    /// Register a bucketed histogram with custom boundaries.
    pub fn register_bucketed(&self, name: &str, boundaries: &[f64]) {
        let mut bucketed = self.bucketed_histograms.write().unwrap();
        bucketed
            .entry(name.to_string())
            .or_insert_with(|| BucketedHistogram::new(boundaries));
    }

    /// Record a bucketed histogram observation.
    pub fn record_bucketed(&self, name: &str, value: f64) {
        let bucketed = self.bucketed_histograms.read().unwrap();
        if let Some(histogram) = bucketed.get(name) {
            histogram.observe(value);
        }
    }

    /// Take a snapshot of all metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let counters = self.counters.read().unwrap();
        let gauges = self.gauges.read().unwrap();
        let histograms = self.histograms.read().unwrap();
        let bucketed = self.bucketed_histograms.read().unwrap();

        MetricsSnapshot {
            counters: counters
                .iter()
                .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
                .collect(),
            gauges: gauges
                .iter()
                .map(|(k, v)| (k.clone(), f64::from_bits(v.load(Ordering::Relaxed))))
                .collect(),
            histograms: histograms
                .iter()
                .map(|(k, v)| (k.clone(), v.to_summary()))
                .collect(),
            bucketed_histograms: bucketed
                .iter()
                .map(|(k, v)| (k.clone(), v.snapshot()))
                .collect(),
        }
    }
}

impl Default for MetricsStore {
    fn default() -> Self {
        Self::new()
    }
}
