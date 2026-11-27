//! Cold-switch coordinator: success and every refusal before disruption.
//! Model-free (fake preparer); real-model switching is a separate gate.

use std::sync::Arc;
use std::time::Duration;

use super::support::*;
use super::*;

#[tokio::test]
async fn a_to_b_to_a_uses_new_handles_and_released_budget() {
    let (lc, limits) = lifecycle(1_000);
    let fake = Fake::with(&[
        ("a.gguf", target(600, "sha-a")),
        ("b.gguf", target(500, "sha-b")),
    ]);
    let a1 = operator_load(&lc, &fake, "a.gguf", "a").await;
    let first = switch(&lc, &fake, request("a", a1, "b.gguf", "b")).await;
    let SwitchOutcome::Switched { to_handle, .. } = first else {
        panic!("{first:?}")
    };
    fake.release("b.gguf");
    assert!(!lc.is_loaded("a").await && lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 500, "A's budget was returned");
    let b = lc.get_handle("b").await.unwrap();
    assert_eq!(b.id(), to_handle);
    assert_eq!(lc.source(b).unwrap().sha256, "sha-b");

    let back = switch(&lc, &fake, request("b", b, "a.gguf", "a")).await;
    assert!(matches!(back, SwitchOutcome::Switched { .. }), "{back:?}");
    let a2 = lc.get_handle("a").await.unwrap();
    assert_ne!(a1, a2, "restored A is a new instance");
    assert_eq!(limits.current_memory(), 600);
    assert!(lc.try_begin_operation().is_some(), "authority released");
}

#[tokio::test]
async fn refusals_before_disruption_leave_a_serving() {
    let (lc, limits) = lifecycle(1_000);
    let fake = Fake::with(&[
        ("a.gguf", target(600, "sha-a")),
        ("b.gguf", target(300, "sha-b")),
    ]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    operator_load(&lc, &fake, "b.gguf", "b").await;
    let stale = crate::models::ModelHandle::new(a.id() + 99);
    let mut too_long = request("a", a, "b.gguf", "c");
    too_long.release_wait = MAX_RELEASE_WAIT + Duration::from_secs(1);
    let cases = [
        (
            request("a", stale, "b.gguf", "c"),
            SwitchRefusal::StaleHandle,
        ),
        (request("x", a, "b.gguf", "c"), SwitchRefusal::NotLoaded),
        (request("a", a, "b.gguf", "b"), SwitchRefusal::TargetLoaded),
        (request("a", a, "a.gguf", "a"), SwitchRefusal::SameModel),
        (too_long, SwitchRefusal::WaitTooLong),
    ];
    for (req, want) in cases {
        assert_eq!(switch(&lc, &fake, req).await, SwitchOutcome::Refused(want));
        assert_eq!(lc.get_handle("a").await, Some(a), "A untouched");
    }
    assert_eq!(limits.current_memory(), 900);
}

#[tokio::test]
async fn a_without_a_recorded_source_is_refused() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[("b.gguf", target(300, "sha-b"))]);
    let meta = crate::models::ModelMetadata {
        name: "a".into(),
        size_bytes: 400,
    };
    let a = lc
        .load("a".into(), meta, Arc::new(Weights(400)))
        .await
        .unwrap();
    let out = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    assert_eq!(out, SwitchOutcome::Refused(SwitchRefusal::UnknownSource));
    assert!(lc.validate_handle(&a).await);
}

#[tokio::test]
async fn over_budget_estimate_and_bad_target_are_refused_before_disruption() {
    let (lc, limits) = lifecycle(1_000);
    let mut bad = target(100, "sha-x");
    bad.preflight_err = Some("prohibited path".into());
    let fake = Fake::with(&[
        ("a.gguf", target(400, "sha-a")),
        ("c.gguf", target(300, "sha-c")),
        ("big.gguf", target(800, "sha-big")),
        ("bad.gguf", bad),
    ]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    operator_load(&lc, &fake, "c.gguf", "c").await;
    assert_eq!(lc.charged_bytes("a").await, 400);
    assert_eq!(lc.charged_bytes("c").await, 300);
    let out = switch(&lc, &fake, request("a", a, "big.gguf", "big")).await;
    let want = SwitchRefusal::OverBudget {
        needed: 800,
        available: 700,
    };
    assert_eq!(out, SwitchOutcome::Refused(want));
    let out = switch(&lc, &fake, request("a", a, "bad.gguf", "bad")).await;
    assert!(
        matches!(out, SwitchOutcome::Refused(SwitchRefusal::Preflight(_))),
        "{out:?}"
    );
    assert!(lc.validate_handle(&a).await);
    assert_eq!(limits.current_memory(), 700);
}

#[tokio::test]
async fn bypass_unload_forgets_the_source() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[("a.gguf", target(400, "sha-a"))]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    assert!(lc.source(a).is_some());
    lc.unload("a").await.unwrap();
    assert!(lc.source(a).is_none(), "every unload path drops the record");
}

#[tokio::test]
async fn unload_expected_refuses_a_replaced_instance() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[("a.gguf", target(400, "sha-a"))]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    let stale = crate::models::ModelHandle::new(a.id() + 1);
    let err = lc.unload_expected("a", stale).await.unwrap_err();
    assert!(matches!(err, LifecycleError::StaleHandle(_)));
    assert!(lc.validate_handle(&a).await);
}

#[tokio::test]
async fn operator_load_records_the_source_and_respects_the_authority() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[("a.gguf", target(400, "sha-a"))]);
    let prep: Arc<dyn Preparer> = Arc::clone(&fake) as Arc<dyn Preparer>;
    let held = lc.try_begin_operation().expect("free");
    let wait = Duration::from_millis(30);
    let busy = load_operated(&lc, &prep, "a.gguf", Some("a".into()), wait).await;
    assert!(matches!(busy, Err(crate::models::ModelOpError::Busy)));
    assert!(!lc.is_loaded("a").await, "nothing loaded while busy");
    drop(held);
    let loaded = load_operated(&lc, &prep, "a.gguf", Some("a".into()), wait)
        .await
        .expect("operator load");
    let handle = crate::models::ModelHandle::new(loaded.handle_id);
    assert_eq!(lc.source(handle).unwrap().sha256, "sha-a");
    assert_eq!(lc.get_handle("a").await, Some(handle));
}

#[tokio::test]
async fn a_source_never_attaches_to_an_unloaded_handle() {
    let (lc, _) = lifecycle(1_000);
    let fake = Fake::with(&[("a.gguf", target(400, "sha-a"))]);
    let a = operator_load(&lc, &fake, "a.gguf", "a").await;
    lc.unload("a").await.unwrap();
    let source = ModelSource {
        relative_path: "a.gguf".into(),
        sha256: "sha-a".into(),
    };
    assert!(!lc.record_source(a, source).await);
    assert!(lc.source(a).is_none());
}
