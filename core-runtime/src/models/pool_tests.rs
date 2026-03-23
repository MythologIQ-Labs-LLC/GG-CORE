//! Tests for the model pool.

use super::*;
use std::time::Duration;

#[tokio::test]
async fn pool_preload_and_switch() {
    let registry = Arc::new(ModelRegistry::new());
    let config = PoolConfig::default();
    let pool = ModelPool::new(config, registry.clone());

    let handle = ModelHandle::new(1);
    pool.preload(
        "qwen-0.5b".to_string(),
        handle,
        ModelTier::Testing,
        500_000_000,
    ).await.unwrap();

    assert!(pool.contains("qwen-0.5b").await);

    let result = pool.switch_to("qwen-0.5b").await.unwrap();
    assert_eq!(result.handle, handle);
    assert!(result.was_preloaded);
    assert!(result.switch_latency < Duration::from_millis(1));
}

#[tokio::test]
async fn pool_eviction_by_tier() {
    let registry = Arc::new(ModelRegistry::new());
    let config = PoolConfig {
        max_models: 2,
        ..Default::default()
    };
    let pool = ModelPool::new(config, registry.clone());

    pool.preload("ci".to_string(), ModelHandle::new(1), ModelTier::Testing, 100).await.unwrap();
    pool.preload("prod".to_string(), ModelHandle::new(2), ModelTier::Quality, 100).await.unwrap();

    // Third model should evict testing tier
    pool.preload("default".to_string(), ModelHandle::new(3), ModelTier::Default, 100).await.unwrap();

    assert!(!pool.contains("ci").await);
    assert!(pool.contains("prod").await);
    assert!(pool.contains("default").await);
}

#[tokio::test]
async fn pool_switch_latency_under_1ms() {
    let registry = Arc::new(ModelRegistry::new());
    let pool = ModelPool::new(PoolConfig::default(), registry.clone());

    pool.preload("test".to_string(), ModelHandle::new(1), ModelTier::Default, 100).await.unwrap();

    for _ in 0..100 {
        let result = pool.switch_to("test").await.unwrap();
        assert!(result.switch_latency < Duration::from_millis(1));
    }
}

#[tokio::test]
async fn pool_warmup_tracking() {
    let registry = Arc::new(ModelRegistry::new());
    let pool = ModelPool::new(PoolConfig::default(), registry.clone());

    pool.preload("test".to_string(), ModelHandle::new(1), ModelTier::Default, 100).await.unwrap();

    let result = pool.switch_to("test").await.unwrap();
    assert!(!result.was_warmed);

    pool.mark_warmed("test").await;

    let result = pool.switch_to("test").await.unwrap();
    assert!(result.was_warmed);
}

#[tokio::test]
async fn pool_switch_emits_latency_metric() {
    let registry = Arc::new(ModelRegistry::new());
    let pool = ModelPool::new(PoolConfig::default(), registry.clone());

    pool.preload("metric-test".to_string(), ModelHandle::new(42), ModelTier::Default, 100)
        .await.unwrap();

    let result = pool.switch_to("metric-test").await.unwrap();
    assert!(result.switch_latency < Duration::from_millis(10));

    let status = pool.status().await;
    assert_eq!(status.metrics.pool_hits, 1);
}
