//! Tests for graceful shutdown coordination.

use gg_core::shutdown::{ShutdownCoordinator, ShutdownResult, ShutdownState};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_initial_state_is_running() {
    let coordinator = ShutdownCoordinator::new();
    assert_eq!(coordinator.state().await, ShutdownState::Running);
}

#[tokio::test]
async fn test_is_accepting_when_running() {
    let coordinator = ShutdownCoordinator::new();
    assert!(coordinator.is_accepting());
}

#[tokio::test]
async fn test_track_increments_count() {
    let coordinator = ShutdownCoordinator::new();
    assert_eq!(coordinator.in_flight_count(), 0);

    let guard = coordinator.track();
    assert!(guard.is_some());
    assert_eq!(coordinator.in_flight_count(), 1);

    drop(guard);
    assert_eq!(coordinator.in_flight_count(), 0);
}

#[tokio::test]
async fn test_multiple_guards_track_correctly() {
    let coordinator = ShutdownCoordinator::new();

    let g1 = coordinator.track();
    let g2 = coordinator.track();
    let g3 = coordinator.track();
    assert_eq!(coordinator.in_flight_count(), 3);

    drop(g1);
    assert_eq!(coordinator.in_flight_count(), 2);

    drop(g2);
    drop(g3);
    assert_eq!(coordinator.in_flight_count(), 0);
}

#[tokio::test]
async fn test_track_returns_none_when_draining() {
    let coordinator = Arc::new(ShutdownCoordinator::new());
    let coord_clone = coordinator.clone();

    // Start drain in background (will wait since no requests)
    let handle = tokio::spawn(async move {
        coord_clone.initiate(Duration::from_millis(100)).await
    });

    // Brief delay for state transition
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Should not accept new requests
    assert!(!coordinator.is_accepting());
    assert!(coordinator.track().is_none());

    let _ = handle.await;
}

#[tokio::test]
async fn test_initiate_transitions_to_draining() {
    let coordinator = Arc::new(ShutdownCoordinator::new());
    let coord_clone = coordinator.clone();

    let handle = tokio::spawn(async move {
        coord_clone.initiate(Duration::from_millis(100)).await
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    let state = coordinator.state().await;
    assert!(state == ShutdownState::Draining || state == ShutdownState::Stopped);

    let _ = handle.await;
}

#[tokio::test]
async fn test_drain_completes_when_zero_requests() {
    let coordinator = ShutdownCoordinator::new();

    let result = coordinator.initiate(Duration::from_millis(100)).await;
    assert!(matches!(result, ShutdownResult::Complete));
}

#[tokio::test]
async fn test_drain_waits_for_completion() {
    let coordinator = Arc::new(ShutdownCoordinator::new());

    // Start a request
    let guard = coordinator.track().unwrap();
    let coord_clone = coordinator.clone();

    let handle = tokio::spawn(async move {
        coord_clone.initiate(Duration::from_millis(500)).await
    });

    // Brief delay, then complete the request
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(guard);

    let result = handle.await.unwrap();
    assert!(matches!(result, ShutdownResult::Complete));
}

#[tokio::test]
async fn test_drain_timeout_returns_remaining() {
    let coordinator = Arc::new(ShutdownCoordinator::new());

    // Start request that won't complete
    let _guard = coordinator.track().unwrap();

    let result = coordinator.initiate(Duration::from_millis(50)).await;
    assert!(matches!(result, ShutdownResult::Timeout { remaining: 1 }));
}

#[tokio::test]
async fn test_state_is_stopped_after_drain() {
    let coordinator = ShutdownCoordinator::new();

    let _ = coordinator.initiate(Duration::from_millis(50)).await;
    assert_eq!(coordinator.state().await, ShutdownState::Stopped);
}
