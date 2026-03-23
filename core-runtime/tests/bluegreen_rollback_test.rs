//! Integration tests for blue-green deployment rollback scenarios.
//!
//! Tests cover instant rollback, failed deployment detection, and state preservation.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

// === Environment Types ===

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvironmentColor {
    Blue,
    Green,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvironmentHealth {
    Healthy,
    Unhealthy,
}

#[allow(dead_code)]
#[derive(Debug)]
struct Environment {
    color: EnvironmentColor,
    health: EnvironmentHealth,
    request_count: AtomicU64,
}

impl Environment {
    fn new(color: EnvironmentColor) -> Self {
        Self {
            color,
            health: EnvironmentHealth::Healthy,
            request_count: AtomicU64::new(0),
        }
    }
}

struct BlueGreenManager {
    blue: Arc<RwLock<Environment>>,
    green: Arc<RwLock<Environment>>,
    active_color: Arc<RwLock<EnvironmentColor>>,
    switch_in_progress: AtomicBool,
}

impl BlueGreenManager {
    fn new() -> Self {
        Self {
            blue: Arc::new(RwLock::new(Environment::new(EnvironmentColor::Blue))),
            green: Arc::new(RwLock::new(Environment::new(EnvironmentColor::Green))),
            active_color: Arc::new(RwLock::new(EnvironmentColor::Blue)),
            switch_in_progress: AtomicBool::new(false),
        }
    }

    async fn switch_traffic(&self) -> Result<EnvironmentColor, &'static str> {
        if self.switch_in_progress.swap(true, Ordering::SeqCst) {
            return Err("Switch already in progress");
        }

        let current = *self.active_color.read().await;
        let target = match current {
            EnvironmentColor::Blue => EnvironmentColor::Green,
            EnvironmentColor::Green => EnvironmentColor::Blue,
        };

        let target_env = match target {
            EnvironmentColor::Blue => self.blue.read().await,
            EnvironmentColor::Green => self.green.read().await,
        };
        if target_env.health == EnvironmentHealth::Unhealthy {
            self.switch_in_progress.store(false, Ordering::SeqCst);
            return Err("Target environment unhealthy");
        }
        drop(target_env);

        *self.active_color.write().await = target;
        self.switch_in_progress.store(false, Ordering::SeqCst);
        Ok(target)
    }

    async fn rollback(&self) -> Result<EnvironmentColor, &'static str> {
        self.switch_traffic().await
    }
}

// === Rollback Tests ===

#[tokio::test]
async fn bluegreen_instant_rollback_trigger() {
    let manager = BlueGreenManager::new();

    // Switch to green
    let _ = manager.switch_traffic().await;
    assert_eq!(*manager.active_color.read().await, EnvironmentColor::Green);

    // Instant rollback to blue
    let result = manager.rollback().await;
    assert!(result.is_ok());
    assert_eq!(*manager.active_color.read().await, EnvironmentColor::Blue);
}

#[tokio::test]
async fn bluegreen_failed_deployment_detection() {
    let manager = BlueGreenManager::new();

    // Simulate deployment to green that fails health check
    {
        let mut green = manager.green.write().await;
        green.health = EnvironmentHealth::Unhealthy;
    }

    // Detect failure before switch
    let green_health = manager.green.read().await.health;
    let deployment_failed = green_health == EnvironmentHealth::Unhealthy;

    assert!(deployment_failed);

    // Should not switch to failed deployment
    let result = manager.switch_traffic().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn bluegreen_automatic_environment_swap() {
    let manager = BlueGreenManager::new();

    // Track switch sequence
    let mut history = vec![*manager.active_color.read().await];

    for _ in 0..4 {
        let _ = manager.switch_traffic().await;
        history.push(*manager.active_color.read().await);
    }

    // Should alternate: Blue -> Green -> Blue -> Green -> Blue
    assert_eq!(
        history,
        vec![
            EnvironmentColor::Blue,
            EnvironmentColor::Green,
            EnvironmentColor::Blue,
            EnvironmentColor::Green,
            EnvironmentColor::Blue,
        ]
    );
}

#[tokio::test]
async fn bluegreen_rollback_preserves_blue_state() {
    let manager = BlueGreenManager::new();

    // Record requests to blue
    for _ in 0..50 {
        manager.blue.read().await.request_count.fetch_add(1, Ordering::Relaxed);
    }

    let blue_before = manager.blue.read().await.request_count.load(Ordering::Relaxed);

    // Switch to green, then rollback
    let _ = manager.switch_traffic().await;
    let _ = manager.rollback().await;

    // Blue state should be preserved
    let blue_after = manager.blue.read().await.request_count.load(Ordering::Relaxed);
    assert_eq!(blue_before, blue_after);
}

#[tokio::test]
async fn bluegreen_multiple_rapid_switches() {
    let manager = Arc::new(BlueGreenManager::new());

    let mut handles = vec![];
    for _ in 0..10 {
        let m = manager.clone();
        handles.push(tokio::spawn(async move { m.switch_traffic().await }));
    }

    let results: Vec<_> = futures::future::join_all(handles).await;

    // All calls should either succeed or fail with known errors
    let successes = results.iter().filter(|r| r.as_ref().unwrap().is_ok()).count();
    let failures = results.iter().filter(|r| r.as_ref().unwrap().is_err()).count();

    // At least one should succeed
    assert!(successes >= 1, "At least one switch should succeed");
    // Total should equal attempts
    assert_eq!(successes + failures, 10);
}

#[tokio::test]
async fn bluegreen_rollback_within_timeout() {
    let manager = BlueGreenManager::new();

    // Switch to green
    let switch_start = std::time::Instant::now();
    let _ = manager.switch_traffic().await;
    let switch_duration = switch_start.elapsed();

    // Rollback
    let rollback_start = std::time::Instant::now();
    let _ = manager.rollback().await;
    let rollback_duration = rollback_start.elapsed();

    // Both operations should be fast (< 100ms)
    assert!(switch_duration < Duration::from_millis(100));
    assert!(rollback_duration < Duration::from_millis(100));
}

#[tokio::test]
async fn bluegreen_double_rollback() {
    let manager = BlueGreenManager::new();

    // Switch to green
    let _ = manager.switch_traffic().await;
    assert_eq!(*manager.active_color.read().await, EnvironmentColor::Green);

    // First rollback to blue
    let _ = manager.rollback().await;
    assert_eq!(*manager.active_color.read().await, EnvironmentColor::Blue);

    // Second rollback to green
    let _ = manager.rollback().await;
    assert_eq!(*manager.active_color.read().await, EnvironmentColor::Green);
}
