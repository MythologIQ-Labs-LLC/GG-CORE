//! S9 / #153 option 2: drain-before-unload with a bounded admission fence.
//!
//! Model-free (fake preparer; the fake's `holders` map stands in for the
//! in-flight requests that hold A). Each test pins one property of the
//! policy: a refused switch leaves A serving under its original handle, a
//! drained switch takes the canonical unload/load path, admissions during
//! the drain wait without leasing and then see the outcome, waiters keep
//! their own deadline and cancellation, an aborted switch lifts the fence,
//! and the budget is never double-charged. The baseline policy is unchanged
//! and remains the default.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::support::*;
use super::*;
use crate::engine::inference::InferenceError;
use crate::engine::{InferenceEngine, InferenceParams, StopControl};

const A: usize = 700;
const B: usize = 500;

/// A (held by one in-flight request) and B, in a 1000-byte budget.
async fn held_a() -> (
    Arc<ModelLifecycle>,
    crate::memory::ResourceLimits,
    Arc<InferenceEngine>,
    Arc<Fake>,
    ModelHandle,
) {
    let (lc, limits, engine) = lifecycle_with_engine(1_000);
    let fake = Fake::with(&[
        ("a.gguf", target(A, "sha-a")),
        ("b.gguf", target(B, "sha-b")),
    ]);
    let a = operator_load_held(&lc, &fake, "a.gguf", "a").await;
    assert_eq!(lc.leases("a").await, Some(1), "the holder is one lease");
    (lc, limits, engine, fake, a)
}

fn drain_request(a: ModelHandle, wait_ms: u64) -> ColdSwitchRequest {
    let mut req = request("a", a, "b.gguf", "b");
    req.release_wait = Duration::from_millis(wait_ms);
    req.policy = ReleasePolicy::DrainBeforeUnload;
    req
}

fn params() -> InferenceParams {
    InferenceParams {
        max_tokens: 1,
        ..Default::default()
    }
}

/// One request through the engine's admission path with its own control.
async fn serve(engine: &InferenceEngine, control: &StopControl) -> Result<(), InferenceError> {
    engine
        .run_controlled("a", "hi", &params(), control, None)
        .await
        .map(|_| ())
}

#[test]
fn the_default_policy_is_the_approved_baseline() {
    assert_eq!(ReleasePolicy::default(), ReleasePolicy::UnloadThenWait);
    let req = request("a", ModelHandle::new(1), "b.gguf", "b");
    assert_eq!(req.policy, ReleasePolicy::UnloadThenWait);
}

#[tokio::test]
async fn drain_timeout_refuses_and_leaves_a_serving_under_its_handle() {
    let (lc, limits, engine, fake, a) = held_a().await;
    let out = switch(&lc, &fake, drain_request(a, 30)).await;
    assert_eq!(
        out,
        SwitchOutcome::Refused(SwitchRefusal::DrainTimedOut { pending: 1 })
    );
    assert_eq!(
        lc.get_handle("a").await,
        Some(a),
        "same instance, same handle"
    );
    assert!(!lc.is_loaded("b").await, "B was never loaded");
    assert_eq!(limits.current_memory(), A, "no duplicate weights charged");
    assert!(
        lc.releasing_models().is_empty(),
        "A was not handed to release"
    );
    assert!(lc.source(a).is_some(), "verified source identity kept");
    assert!(lc.try_begin_operation().is_some(), "authority released");
    serve(&engine, &StopControl::default())
        .await
        .expect("A serves after the refusal");
    assert_eq!(lc.leases("a").await, Some(1), "only the original holder");
}

#[tokio::test]
async fn drained_switch_unloads_a_and_loads_b_on_the_canonical_path() {
    let (lc, limits, _engine, fake, a) = held_a().await;
    let task = {
        let (lc, fake) = (Arc::clone(&lc), Arc::clone(&fake));
        tokio::spawn(async move { switch(&lc, &fake, drain_request(a, 2_000)).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!task.is_finished(), "waits for the lease to end");
    assert_eq!(
        lc.get_handle("a").await,
        Some(a),
        "A still registered while draining"
    );
    fake.release("a.gguf"); // the in-flight request ends
    let out = task.await.expect("joined");
    let SwitchOutcome::Switched {
        from_handle,
        to_handle,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(from_handle, a.id());
    assert_eq!(lc.get_handle("b").await.map(|h| h.id()), Some(to_handle));
    assert!(!lc.is_loaded("a").await);
    assert_eq!(
        limits.current_memory(),
        B,
        "A's budget returned, B charged once"
    );
    assert!(lc.releasing_models().is_empty(), "A released synchronously");
    assert_eq!(
        lc.source(ModelHandle::new(to_handle)).unwrap().sha256,
        "sha-b"
    );
}

#[tokio::test]
async fn admissions_during_the_drain_wait_and_then_run_on_a_after_a_refusal() {
    let (lc, _limits, engine, fake, a) = held_a().await;
    let task = {
        let (lc, fake) = (Arc::clone(&lc), Arc::clone(&fake));
        tokio::spawn(async move { switch(&lc, &fake, drain_request(a, 300)).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    let waiters: Vec<_> = (0..3)
        .map(|_| {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                let control = StopControl::from_timeout(None, Some(5_000));
                serve(&engine, &control).await
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(waiters.iter().all(|w| !w.is_finished()), "fenced: all wait");
    assert_eq!(lc.leases("a").await, Some(1), "waiters hold no lease");
    let out = task.await.expect("joined");
    assert!(matches!(
        out,
        SwitchOutcome::Refused(SwitchRefusal::DrainTimedOut { pending: 1 })
    ));
    for w in waiters {
        w.await
            .expect("joined")
            .expect("served by A after the refusal");
    }
}

#[tokio::test]
async fn admissions_during_a_successful_drain_see_model_not_loaded() {
    let (lc, _limits, engine, fake, a) = held_a().await;
    let task = {
        let (lc, fake) = (Arc::clone(&lc), Arc::clone(&fake));
        tokio::spawn(async move { switch(&lc, &fake, drain_request(a, 2_000)).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    let waiter = {
        let engine = Arc::clone(&engine);
        tokio::spawn(async move {
            let control = StopControl::from_timeout(None, Some(5_000));
            serve(&engine, &control).await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiter.is_finished());
    fake.release("a.gguf");
    assert!(matches!(
        task.await.expect("joined"),
        SwitchOutcome::Switched { .. }
    ));
    assert!(matches!(
        waiter.await.expect("joined"),
        Err(InferenceError::ModelNotLoaded(_))
    ));
}

#[tokio::test]
async fn waiting_admissions_keep_their_own_deadline_and_cancellation() {
    let (lc, _limits, engine, fake, a) = held_a().await;
    let task = {
        let (lc, fake) = (Arc::clone(&lc), Arc::clone(&fake));
        tokio::spawn(async move { switch(&lc, &fake, drain_request(a, 1_000)).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    let started = std::time::Instant::now();
    let short = StopControl::from_timeout(None, Some(60));
    assert!(matches!(
        serve(&engine, &short).await,
        Err(InferenceError::DeadlineExceeded { timeout_ms: 60 })
    ));
    let flag = Arc::new(AtomicBool::new(false));
    let cancelled = StopControl::from_timeout(Some(Arc::clone(&flag)), Some(5_000));
    let waiter = {
        let engine = Arc::clone(&engine);
        tokio::spawn(async move { serve(&engine, &cancelled).await })
    };
    tokio::time::sleep(Duration::from_millis(40)).await;
    flag.store(true, Ordering::Release);
    assert!(matches!(
        waiter.await.expect("joined"),
        Err(InferenceError::Cancelled)
    ));
    assert!(
        started.elapsed() < Duration::from_millis(700),
        "neither waited for the drain to expire"
    );
    assert!(matches!(
        task.await.expect("joined"),
        SwitchOutcome::Refused(SwitchRefusal::DrainTimedOut { pending: 1 })
    ));
}

#[tokio::test]
async fn an_aborted_switch_lifts_the_fence_and_a_serves() {
    let (lc, _limits, engine, fake, a) = held_a().await;
    let prep: Arc<dyn Preparer> = Arc::clone(&fake) as Arc<dyn Preparer>;
    let task = spawn_cold_switch(Arc::clone(&lc), prep, drain_request(a, 5_000)).expect("spawned");
    tokio::time::sleep(Duration::from_millis(50)).await;
    task.abort();
    assert!(matches!(
        join_switch(task).await,
        SwitchOutcome::Unknown { .. }
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(lc.try_begin_operation().is_some(), "authority released");
    assert_eq!(lc.get_handle("a").await, Some(a));
    let served = tokio::time::timeout(
        Duration::from_secs(2),
        serve(&engine, &StopControl::default()),
    )
    .await;
    assert!(
        matches!(served, Ok(Ok(()))),
        "fence lifted by RAII: {served:?}"
    );
}

#[tokio::test]
async fn a_stale_handle_cannot_fence_a_replacement() {
    let (lc, _limits, engine, fake, a) = held_a().await;
    fake.release("a.gguf");
    lc.unload("a").await.expect("unload a");
    let a2 = operator_load(&lc, &fake, "a.gguf", "a").await;
    assert_ne!(a, a2);
    assert!(matches!(
        lc.fence_admissions("a", a).await,
        Err(LifecycleError::StaleHandle(_))
    ));
    assert!(matches!(
        lc.fence_admissions("x", a2).await,
        Err(LifecycleError::NotLoaded(_))
    ));
    serve(&engine, &StopControl::default())
        .await
        .expect("no fence was raised");
    let out = switch(&lc, &fake, drain_request(a, 100)).await;
    assert_eq!(out, SwitchOutcome::Refused(SwitchRefusal::StaleHandle));
}

#[tokio::test]
async fn the_ceiling_holds_during_and_after_a_refused_drain() {
    let (lc, limits, _engine, fake, a) = held_a().await;
    let task = {
        let (lc, fake) = (Arc::clone(&lc), Arc::clone(&fake));
        tokio::spawn(async move { switch(&lc, &fake, drain_request(a, 200)).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(limits.current_memory(), A, "only A charged while draining");
    assert!(matches!(
        task.await.expect("joined"),
        SwitchOutcome::Refused(SwitchRefusal::DrainTimedOut { pending: 1 })
    ));
    let prep: Arc<dyn Preparer> = Arc::clone(&fake) as Arc<dyn Preparer>;
    assert!(
        load_checked(&lc, &prep, "b.gguf", "b").await.is_err(),
        "700 + 500 must still not fit the 1000-byte ceiling"
    );
    assert_eq!(
        limits.current_memory(),
        A,
        "a refused load leaves no charge"
    );
    assert_eq!(lc.get_handle("a").await, Some(a));
}

#[tokio::test]
async fn an_unheld_a_drains_at_once_and_switches() {
    let (lc, limits, _engine, fake, a) = held_a().await;
    fake.release("a.gguf");
    assert_eq!(lc.leases("a").await, Some(0));
    let out = switch(&lc, &fake, drain_request(a, 1)).await;
    assert!(matches!(out, SwitchOutcome::Switched { .. }), "{out:?}");
    assert_eq!(limits.current_memory(), B);
}
