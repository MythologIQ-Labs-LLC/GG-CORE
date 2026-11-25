//! S9 / #153: a cold switch whose release wait expires (`ReleasePending`).
//!
//! Pins the current, documented outage without redefining it: A is closed,
//! B is not loaded, no budget is double-charged or left stale, the ceiling
//! still refuses B while A's bytes are held, and an explicit operator load
//! recovers once A has really released. Model-free (fake preparer).

use std::sync::Arc;
use std::time::Duration;

use super::support::*;
use super::*;
use crate::memory::ResourceLimits;

/// Switch A (700 of 1000 bytes, held by an in-flight request) to B (500)
/// with a release wait that expires while A is still held.
async fn pending_switch() -> (Arc<ModelLifecycle>, ResourceLimits, Arc<Fake>, ModelHandle) {
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
    (lc, limits, fake, a)
}

async fn released(lc: &ModelLifecycle, limits: &ResourceLimits) {
    assert_eq!(
        lc.wait_released("a", Duration::from_secs(5)).await,
        (true, 0)
    );
    assert_eq!(limits.current_memory(), 0, "stale charge after release");
}

#[tokio::test]
async fn pending_release_is_an_outage_that_frees_the_authority() {
    let (lc, limits, fake, a) = pending_switch().await;
    assert!(!lc.is_loaded("a").await, "A must stay closed to admissions");
    assert!(
        !lc.is_loaded("b").await,
        "B must not load over held A bytes"
    );
    assert_eq!(
        limits.current_memory(),
        700,
        "only A's held bytes are charged"
    );
    assert!(lc.try_begin_operation().is_some(), "authority released");
    // A second switch cannot run: A no longer serves, so it is refused
    // before any disruption rather than queued behind the release.
    let again = switch(&lc, &fake, request("a", a, "b.gguf", "b")).await;
    assert_eq!(again, SwitchOutcome::Refused(SwitchRefusal::NotLoaded));
    fake.release("a.gguf");
    released(&lc, &limits).await;
}

#[tokio::test]
async fn ceiling_refuses_b_while_a_bytes_are_still_held() {
    let (lc, limits, fake, _a) = pending_switch().await;
    let prep: Arc<dyn Preparer> = Arc::clone(&fake) as Arc<dyn Preparer>;
    assert!(
        load_checked(&lc, &prep, "b.gguf", "b").await.is_err(),
        "700 held + 500 must not exceed the 1000-byte ceiling"
    );
    assert!(!lc.is_loaded("b").await);
    assert_eq!(
        limits.current_memory(),
        700,
        "a refused load leaves no charge"
    );
    fake.release("a.gguf");
    released(&lc, &limits).await;
}

#[tokio::test]
async fn explicit_load_recovers_either_model_after_real_release() {
    let (lc, limits, fake, a) = pending_switch().await;
    fake.release("a.gguf");
    released(&lc, &limits).await;
    let b = operator_load(&lc, &fake, "b.gguf", "b").await;
    assert!(lc.is_loaded("b").await);
    assert_eq!(limits.current_memory(), 500, "B charged alone");
    assert_ne!(b, a, "B is a new instance");
    lc.unload("b").await.expect("unload b");
    assert_eq!(
        lc.wait_released("b", Duration::from_secs(5)).await,
        (true, 0)
    );
    let a2 = operator_load(&lc, &fake, "a.gguf", "a").await;
    assert_ne!(a2, a, "A returns as a new instance, never the released one");
    assert_eq!(limits.current_memory(), 700);
}
