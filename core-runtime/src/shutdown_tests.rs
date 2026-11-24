//! Shutdown admission and native-work drain policy regressions (#219).

use super::{ShutdownCoordinator, ShutdownResult, ShutdownState};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_guard_is_charged_until_release_and_new_work_is_refused() {
    let coordinator = Arc::new(ShutdownCoordinator::new());
    let guard = coordinator
        .track()
        .expect("running coordinator accepts work");
    assert_eq!(coordinator.in_flight_count(), 1);
    let waiter = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.initiate(Duration::from_secs(15)).await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while coordinator.state().await != ShutdownState::Draining {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shutdown reaches Draining");
    assert!(
        coordinator.track().is_none(),
        "no admission after drain begins"
    );
    assert_eq!(coordinator.in_flight_count(), 1);
    assert!(
        !waiter.is_finished(),
        "cannot finish while actual guard exists"
    );
    drop(guard);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("released guard wakes draining shutdown")
            .expect("shutdown task joins"),
        ShutdownResult::Complete
    );
    assert_eq!(coordinator.state().await, ShutdownState::Stopped);
    assert!(
        coordinator.track().is_none(),
        "stopped coordinator stays closed"
    );
    assert_eq!(coordinator.in_flight_count(), 0);
}

#[tokio::test]
async fn write_locked_state_cannot_admit_a_new_guard() {
    let coordinator = ShutdownCoordinator::new();
    let writer = coordinator.state.write().await;
    assert!(!coordinator.is_accepting());
    assert!(coordinator.track().is_none());
    assert_eq!(coordinator.in_flight_count(), 0);
    drop(writer);
    let guard = coordinator.track().expect("admission resumes after writer");
    assert_eq!(coordinator.in_flight_count(), 1);
    drop(guard);
    assert_eq!(coordinator.in_flight_count(), 0);
}

#[tokio::test]
async fn expired_shutdown_never_reports_all_native_work_released() {
    let coordinator = ShutdownCoordinator::new();
    let guard = coordinator.track().expect("admit work");
    assert_eq!(
        coordinator.initiate(Duration::ZERO).await,
        ShutdownResult::Timeout { remaining: 1 }
    );
    assert_eq!(coordinator.state().await, ShutdownState::Stopped);
    assert!(coordinator.track().is_none());
    assert_eq!(coordinator.in_flight_count(), 1);
    drop(guard);
    assert_eq!(coordinator.in_flight_count(), 0);
}
