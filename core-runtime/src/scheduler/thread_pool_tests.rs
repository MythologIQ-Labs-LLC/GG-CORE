//! Tests for the configurable thread pool.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Wait (bounded) for the pool to report `n` executed tasks. A fixed sleep
/// races the workers on a loaded host; the assertion itself is unchanged.
fn wait_for_executed(pool: &ThreadPool, n: u64) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let executed = pool.stats().total_tasks_executed;
        if executed >= n || Instant::now() >= deadline {
            return executed;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn test_thread_pool_basic() {
    let pool = ThreadPool::new(ThreadPoolConfig::default());
    assert!(pool.num_threads() > 0);

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    pool.submit(Box::new(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }))
    .unwrap();

    wait_for_executed(&pool, 1);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn test_priority_tasks() {
    let config = ThreadPoolConfig {
        enable_priority: true,
        ..Default::default()
    };
    let pool = ThreadPool::new(config);

    let results = Arc::new(Mutex::new(Vec::new()));

    for i in 0..3 {
        let results_clone = results.clone();
        pool.submit_with_priority(
            Box::new(move || {
                results_clone.lock().unwrap().push(format!("normal-{}", i));
            }),
            TaskPriority::Normal,
        )
        .unwrap();
    }

    pool.submit_with_priority(Box::new(|| {}), TaskPriority::High)
        .unwrap();

    assert!(wait_for_executed(&pool, 3) >= 3);
}

#[test]
fn test_config_presets() {
    let inference_config = ThreadPoolConfig::inference_optimized();
    assert!(inference_config.enable_work_stealing);
    assert!(inference_config.enable_priority);

    let batch_config = ThreadPoolConfig::batch_optimized();
    assert!(batch_config.enable_work_stealing);
    assert!(!batch_config.enable_priority);
}

#[test]
fn test_stats_tracking() {
    let pool = ThreadPool::new(ThreadPoolConfig::default());

    for _ in 0..10 {
        pool.submit(Box::new(|| {})).unwrap();
    }

    assert!(wait_for_executed(&pool, 10) >= 10);
}
