//! Standalone cold-switch safety prototype (#137 / #153).
//!
//! Exercises the *actual* lifecycle and resident accounting with fake weights.
//! This is not a controller, a real-model proof, or authorization to evict
//! arbitrary in-flight requests. An independently authenticated switch
//! operation and an owner-approved rollback policy are still required.

use std::sync::Arc;
use std::time::Duration;

use super::resident_tests::{meta, setup, Weights};
use super::*;
use crate::engine::Model;

#[tokio::test]
async fn cold_a_to_b_to_a_waits_for_held_weights_and_preserves_identity() {
    let (lifecycle, limits) = setup(1_000);
    let held: Arc<dyn Model> = Arc::new(Weights(700));
    let first = lifecycle
        .load("a".into(), meta("a", 700), Arc::clone(&held))
        .await
        .expect("load first source");

    lifecycle.unload("a").await.expect("start drain of A");
    assert!(!lifecycle.is_loaded("a").await);
    assert!(!lifecycle.validate_handle(&first).await);
    assert_eq!(limits.current_memory(), 700, "in-flight A still charged");
    assert_eq!(
        lifecycle.wait_released("a", Duration::ZERO).await,
        (false, 1),
        "unload acknowledgement is not proof of memory release"
    );

    let rejected = lifecycle
        .load("b".into(), meta("b", 500), Arc::new(Weights(500)))
        .await
        .expect_err("A's held 700 bytes must block B's 500-byte admission");
    assert!(matches!(rejected, LifecycleError::ResourceExhausted(_)));
    assert!(!lifecycle.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 700);

    drop(held);
    assert_eq!(
        lifecycle.wait_released("a", Duration::from_secs(5)).await,
        (true, 0),
        "only the final holder may release A"
    );
    assert_eq!(limits.current_memory(), 0);

    let second = lifecycle
        .load("b".into(), meta("b", 500), Arc::new(Weights(500)))
        .await
        .expect("B may load only after A actually released");
    assert_ne!(first.id(), second.id(), "handles are never reused");
    assert!(lifecycle.validate_handle(&second).await);
    assert_eq!(limits.current_memory(), 500);

    lifecycle.unload("b").await.expect("unload B");
    assert_eq!(
        lifecycle.wait_released("b", Duration::from_secs(5)).await,
        (true, 0)
    );
    assert_eq!(limits.current_memory(), 0);

    let restored = lifecycle
        .load("a".into(), meta("a", 700), Arc::new(Weights(700)))
        .await
        .expect("return to A under the original budget");
    assert_ne!(first.id(), restored.id(), "restored A is a new instance");
    assert_ne!(second.id(), restored.id());
    assert!(lifecycle.validate_handle(&restored).await);
    assert!(!lifecycle.validate_handle(&first).await);
    assert_eq!(limits.current_memory(), 700);
}

#[tokio::test]
async fn speculative_warm_replacement_cannot_evict_current_model_to_fit() {
    let (lifecycle, limits) = setup(1_000);
    let first = lifecycle
        .load("a".into(), meta("a", 800), Arc::new(Weights(800)))
        .await
        .expect("load A");

    let failure = lifecycle
        .load("b".into(), meta("b", 300), Arc::new(Weights(300)))
        .await
        .expect_err("overlapping A+B exceeds 1000");
    assert!(matches!(failure, LifecycleError::ResourceExhausted(_)));
    assert!(lifecycle.validate_handle(&first).await);
    assert!(lifecycle.is_loaded("a").await);
    assert!(!lifecycle.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 800);
}

#[tokio::test]
async fn release_timeout_cannot_be_interpreted_as_permission_to_reload() {
    let (lifecycle, limits) = setup(1_000);
    let holder: Arc<dyn Model> = Arc::new(Weights(700));
    lifecycle
        .load("a".into(), meta("a", 700), Arc::clone(&holder))
        .await
        .unwrap();
    lifecycle.unload("a").await.unwrap();

    let (released, pending) = lifecycle
        .wait_released("a", Duration::from_millis(10))
        .await;
    assert!(!released);
    assert_eq!(pending, 1);
    let failure = lifecycle
        .load("b".into(), meta("b", 400), Arc::new(Weights(400)))
        .await
        .expect_err("timed-out wait cannot bypass the budget");
    assert!(matches!(failure, LifecycleError::ResourceExhausted(_)));
    assert_eq!(limits.current_memory(), 700);
    drop(holder);
    assert_eq!(
        lifecycle.wait_released("a", Duration::from_secs(5)).await,
        (true, 0)
    );
}
