//! Cold-switch coordinator: release timeouts, recovery, rollback binding,
//! concurrency and task-failure reporting. Model-free (fake preparer).

use std::sync::Arc;
use std::time::Duration;

use super::support::*;
use super::*;

#[tokio::test]
async fn release_timeout_reports_pending_without_loading_b() {
    let (lc, limits) = lifecycle(1_000);
    let fake = Fake::with(&[
        ("a.gguf", target(700, "sha-a")),
        ("b.gguf", target(500, "sha-b")),
    ]);
    let a = operator_load_held(&lc, &fake, "a.gguf", "a").await;
    let mut req = request("a", a, "b.gguf", "b");
    req.release_wait = Duration::from_millis(20);
    let out = switch(&lc, &fake, req).await;
    assert_eq!(
        out,
        SwitchOutcome::ReleasePending {
            from_handle: a.id(),
            pending: 1
        }
    );
    assert!(!lc.is_loaded("a").await && !lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 700, "no double charge");
    fake.release("a.gguf"); // the in-flight request finishes
    assert_eq!(
        lc.wait_released("a", Duration::from_secs(2)).await,
        (true, 0)
    );
}

#[tokio::test]
async fn b_prepare_failure_restores_a_from_its_verified_source() {
    let (lc, limits) = lifecycle(1_000);
    let mut broken = target(500, "sha-b");
    broken.prepare_err = Some("backend refused the file".into());
    let fake = Fake::with(&[("a.gguf", target(700, "sha-a")), ("b.gguf", broken)]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let out = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    let SwitchOutcome::RolledBack {
        restored_handle, ..
    } = out
    else {
        panic!("{out:?}")
    };
    let restored = lc.get_handle("a").await.unwrap();
    assert_eq!(restored.id(), restored_handle);
    assert_ne!(restored, a, "never a reused handle");
    assert_eq!(lc.source(restored).unwrap().sha256, "sha-a");
    assert!(!lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 700);
}

#[tokio::test]
async fn register_resize_refusal_rolls_back() {
    let (lc, limits) = lifecycle(1_000);
    let mut grows = target(300, "sha-b");
    grows.memory = 1_200; // fits the reservation, not the resized footprint
    let fake = Fake::with(&[("a.gguf", target(700, "sha-a")), ("b.gguf", grows)]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let out = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    assert!(matches!(out, SwitchOutcome::RolledBack { .. }), "{out:?}");
    assert!(lc.is_loaded("a").await && !lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 700);
}

#[tokio::test]
async fn b_failing_readiness_is_retired_before_a_returns() {
    let (lc, limits) = lifecycle(1_000);
    let mut sick = target(500, "sha-b");
    sick.ready_err = Some("smoke check failed".into());
    sick.held = false; // nothing else holds B once it is unloaded
    let fake = Fake::with(&[("a.gguf", target(400, "sha-a")), ("b.gguf", sick)]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let out = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    assert!(matches!(out, SwitchOutcome::RolledBack { .. }), "{out:?}");
    assert!(lc.is_loaded("a").await && !lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 400);
}

#[tokio::test]
async fn recovery_release_timeout_is_reported_not_double_charged() {
    let (lc, limits) = lifecycle(1_000);
    let mut sick = target(500, "sha-b");
    sick.ready_err = Some("smoke check failed".into());
    let fake = Fake::with(&[("a.gguf", target(400, "sha-a")), ("b.gguf", sick)]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let mut req = request("a", a, "b.gguf", "b");
    req.release_wait = Duration::from_millis(50); // fake keeps holding B
    let out = switch(&lc, &fake, req).await;
    let SwitchOutcome::RollbackFailed { rollback_error, .. } = out else {
        panic!("{out:?}")
    };
    assert!(
        rollback_error.contains("target release pending: 1"),
        "{rollback_error}"
    );
    assert!(!lc.is_loaded("a").await && !lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 500, "only B's held bytes");
}

#[tokio::test]
async fn changed_source_is_never_restored() {
    let (lc, limits) = lifecycle(1_000);
    let mut broken = target(500, "sha-b");
    broken.prepare_err = Some("backend refused the file".into());
    let fake = Fake::with(&[("a.gguf", target(700, "sha-a")), ("b.gguf", broken)]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    fake.set("a.gguf", target(700, "sha-SUBSTITUTED"));
    let out = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    let SwitchOutcome::RollbackFailed { rollback_error, .. } = out else {
        panic!("{out:?}")
    };
    assert!(
        rollback_error.contains("sha256 mismatch"),
        "{rollback_error}"
    );
    assert!(!lc.is_loaded("a").await && !lc.is_loaded("b").await);
    fake.release("a.gguf");
    assert_eq!(
        limits.current_memory(),
        0,
        "the refused reload returned its budget"
    );
}

#[tokio::test]
async fn concurrent_operators_are_refused_or_bounded() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[
        ("a.gguf", target(400, "sha-a")),
        ("b.gguf", target(300, "sha-b")),
    ]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let held = lc.try_begin_operation().expect("authority free");
    let out = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    assert_eq!(out, SwitchOutcome::Refused(SwitchRefusal::Busy));
    let wait = Duration::from_millis(30);
    assert!(
        lc.begin_operation_within(wait).await.is_none(),
        "bounded wait"
    );
    let unload = crate::models::unload_model_operated(&lc, "a", wait).await;
    assert!(matches!(unload, Err(crate::models::ModelOpError::Busy)));
    assert!(lc.validate_handle(&a).await);
    drop(held);
    assert!(lc.try_begin_operation().is_some());
}

#[tokio::test]
async fn a_dropped_caller_does_not_interrupt_the_switch() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[
        ("a.gguf", target(400, "sha-a")),
        ("b.gguf", target(300, "sha-b")),
    ]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let prep: Arc<dyn Preparer> = Arc::clone(&fake) as Arc<dyn Preparer>;
    let handle = spawn_cold_switch(Arc::clone(&lc), prep, request("a", a, "b.gguf", "b"));
    drop(handle.expect("spawned"));
    for _ in 0..200 {
        if lc.is_loaded("b").await && lc.try_begin_operation().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the detached switch did not complete and release the authority");
}

#[tokio::test]
async fn an_abnormal_task_end_reports_unknown_state() {
    let task = tokio::spawn(async { panic!("simulated coordinator failure") });
    let out = join_switch(task).await;
    let SwitchOutcome::Unknown { reason } = out else {
        panic!("{out:?}")
    };
    assert!(reason.contains("state is unknown"), "{reason}");
}
