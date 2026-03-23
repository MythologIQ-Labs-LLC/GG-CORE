//! Integration tests for SwapManager - zero-downtime model hot-swap.

use std::sync::Arc;
use std::time::Duration;
use gg_core::models::{
    FlightTracker, ModelArchitecture, ModelCapability, ModelManifest, ModelRegistry, ModelRouter,
    SwapError, SwapManager,
};

fn test_manifest(model_id: &str) -> ModelManifest {
    ModelManifest {
        model_id: model_id.to_string(),
        name: format!("{} Model", model_id),
        version: "1.0.0".to_string(),
        capabilities: vec![ModelCapability::TextGeneration],
        sha256: "a".repeat(64),
        size_bytes: 1024,
        architecture: ModelArchitecture::Gguf,
        license: "MIT".to_string(),
    }
}

async fn setup_swap_manager() -> (
    SwapManager,
    Arc<ModelRegistry>,
    Arc<ModelRouter>,
    Arc<FlightTracker>,
) {
    let registry = Arc::new(ModelRegistry::new());
    let router = Arc::new(ModelRouter::new());
    let flight_tracker = Arc::new(FlightTracker::new());
    let manager = SwapManager::new(registry.clone(), router.clone(), flight_tracker.clone());
    (manager, registry, router, flight_tracker)
}

#[tokio::test]
async fn test_zero_downtime_swap() {
    let (manager, registry, router, _) = setup_swap_manager().await;

    // Setup initial model
    let old_handle = registry
        .register(
            gg_core::models::ModelMetadata {
                name: "old-model".to_string(),
                size_bytes: 1024,
            },
            1024,
        )
        .await;
    router.add_route("test-model", old_handle).await.unwrap();

    assert!(router.resolve("test-model").await == Some(old_handle));

    // Execute swap
    let new_manifest = test_manifest("test-model");
    let result = manager
        .execute_swap("test-model", new_manifest, Duration::from_millis(100))
        .await;

    assert!(result.is_ok());
    let swap_result = result.unwrap();

    assert_eq!(swap_result.old_handle, old_handle);
    assert_ne!(swap_result.new_handle, old_handle);

    // Verify route points to new handle
    let resolved = router.resolve("test-model").await;
    assert_eq!(resolved, Some(swap_result.new_handle));

    // Verify old model unregistered
    assert!(!registry.contains(old_handle).await);
    // Verify new model registered
    assert!(registry.contains(swap_result.new_handle).await);
}

#[tokio::test]
async fn test_swap_with_drain_timeout() {
    let (manager, registry, router, flight_tracker) = setup_swap_manager().await;

    // Setup initial model
    let old_handle = registry
        .register(
            gg_core::models::ModelMetadata {
                name: "old-model".to_string(),
                size_bytes: 1024,
            },
            1024,
        )
        .await;
    router.add_route("test-model", old_handle).await.unwrap();

    // Simulate in-flight request that won't complete
    let _guard = flight_tracker.track(old_handle).await;

    // Execute swap with short timeout
    let new_manifest = test_manifest("test-model");
    let result = manager
        .execute_swap("test-model", new_manifest, Duration::from_millis(50))
        .await;

    assert!(matches!(result, Err(SwapError::DrainTimeout)));

    // Verify old model still routed
    assert_eq!(router.resolve("test-model").await, Some(old_handle));
    // Verify old model still registered
    assert!(registry.contains(old_handle).await);
}

#[tokio::test]
async fn test_swap_route_not_found() {
    let (manager, _, _, _) = setup_swap_manager().await;

    let new_manifest = test_manifest("nonexistent");
    let result = manager
        .execute_swap("nonexistent", new_manifest, Duration::from_millis(100))
        .await;

    assert!(matches!(result, Err(SwapError::RouteNotFound(_))));
}

#[tokio::test]
async fn test_swap_preload_failure_rollback() {
    let (manager, registry, router, _) = setup_swap_manager().await;

    // Setup initial model
    let old_handle = registry
        .register(
            gg_core::models::ModelMetadata {
                name: "old-model".to_string(),
                size_bytes: 1024,
            },
            1024,
        )
        .await;
    router.add_route("test-model", old_handle).await.unwrap();

    // Try swap with invalid manifest
    let mut bad_manifest = test_manifest("test-model");
    bad_manifest.model_id = "".to_string(); // Invalid

    let result = manager
        .execute_swap("test-model", bad_manifest, Duration::from_millis(100))
        .await;

    assert!(matches!(result, Err(SwapError::PreloadFailed(_))));

    // Verify old model still routed and registered
    assert_eq!(router.resolve("test-model").await, Some(old_handle));
    assert!(registry.contains(old_handle).await);
}

#[tokio::test]
async fn test_concurrent_swap_rejected() {
    let (manager, registry, router, flight_tracker) = setup_swap_manager().await;

    // Setup initial model
    let old_handle = registry
        .register(
            gg_core::models::ModelMetadata {
                name: "old-model".to_string(),
                size_bytes: 1024,
            },
            1024,
        )
        .await;
    router.add_route("test-model", old_handle).await.unwrap();

    // Start a swap that will be blocked by in-flight request
    let _guard = flight_tracker.track(old_handle).await;

    let manager = Arc::new(manager);
    let manager1 = manager.clone();

    // First swap (will be blocked waiting for drain)
    let swap1 = tokio::spawn(async move {
        manager1
            .execute_swap(
                "test-model",
                test_manifest("test-model"),
                Duration::from_millis(500),
            )
            .await
    });

    // Brief delay to ensure first swap has started
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Second swap should be rejected
    let result2 = manager
        .execute_swap(
            "test-model",
            test_manifest("test-model"),
            Duration::from_millis(100),
        )
        .await;

    assert!(matches!(result2, Err(SwapError::SwapInProgress)));

    // Cleanup: timeout first swap
    let _ = swap1.await;
}

#[tokio::test]
async fn test_swap_manager_is_idle_after_completion() {
    let (manager, registry, router, _) = setup_swap_manager().await;

    // Setup initial model
    let old_handle = registry
        .register(
            gg_core::models::ModelMetadata {
                name: "old-model".to_string(),
                size_bytes: 1024,
            },
            1024,
        )
        .await;
    router.add_route("test-model", old_handle).await.unwrap();

    assert!(manager.is_idle().await);

    let _ = manager
        .execute_swap(
            "test-model",
            test_manifest("test-model"),
            Duration::from_millis(100),
        )
        .await;

    assert!(manager.is_idle().await);
}
