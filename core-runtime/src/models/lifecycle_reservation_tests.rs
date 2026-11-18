//! Pre-allocation reservations: concurrent loads, rollback on backend
//! failure, cancellation of an abandoned load, and deferred release while an
//! in-flight request still holds an unloaded model.

use std::sync::Arc;
use std::time::Duration;

use super::{meta, setup, Weights};
use crate::engine::Model;
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::{
    prepare_model_admitted, register_prepared, LifecycleError, ModelLoader, ModelOpError,
    PreparedModel,
};

fn limits(total: usize) -> ResourceLimits {
    ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 10,
        max_total_memory: total,
        max_concurrent: 2,
    })
}

/// A confined models/ tree holding one invalid (non-GGUF) file of `size`.
fn invalid_model(size: usize) -> (tempfile::TempDir, ModelLoader) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    std::fs::write(dir.path().join("models/m.gguf"), vec![0u8; size]).unwrap();
    let loader = ModelLoader::new(dir.path().to_path_buf());
    (dir, loader)
}

fn is_exhausted(err: &ModelOpError) -> bool {
    matches!(
        err,
        ModelOpError::Lifecycle(LifecycleError::ResourceExhausted(_))
    )
}

#[test]
fn second_concurrent_load_is_refused_before_backend_allocation() {
    // The headroom check this replaces did not reserve: two 600-byte loads
    // under a 1000-byte budget both passed and both allocated. Now the first
    // in-flight load holds its 600 bytes while the backend runs.
    let l = limits(1_000);
    let in_flight = l.reserve_resident(600).expect("first load reserves");
    let (_dir, loader) = invalid_model(600);
    let err = prepare_model_admitted(&loader, Some(&l), "models/m.gguf", None)
        .err()
        .expect("second load must not fit");
    assert!(
        is_exhausted(&err),
        "refused by admission, not backend: {err}"
    );
    assert_eq!(l.current_memory(), 600);
    drop(in_flight);
    assert_eq!(l.current_memory(), 0);
}

#[test]
fn parallel_prepares_never_exceed_the_total() {
    let l = limits(1_000);
    let (_dir, loader) = invalid_model(600);
    let loader = Arc::new(loader);
    let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (l, loader, peak) = (l.clone(), Arc::clone(&loader), Arc::clone(&peak));
            std::thread::spawn(move || {
                let r = prepare_model_admitted(&loader, Some(&l), "models/m.gguf", None);
                peak.fetch_max(l.current_memory(), std::sync::atomic::Ordering::SeqCst);
                r.err().map(|e| is_exhausted(&e))
            })
        })
        .collect();
    for h in handles {
        // Every load fails (invalid file): either admission or backend.
        assert!(h.join().unwrap().is_some());
    }
    assert!(peak.load(std::sync::atomic::Ordering::SeqCst) <= 1_000);
    assert_eq!(l.current_memory(), 0, "every reservation rolled back");
}

#[test]
fn backend_failure_after_reservation_rolls_back() {
    let l = limits(10_000);
    let (_dir, loader) = invalid_model(4_096);
    let err = prepare_model_admitted(&loader, Some(&l), "models/m.gguf", None)
        .err()
        .expect("invalid GGUF fails in the backend");
    assert!(matches!(err, ModelOpError::Backend(_)), "{err}");
    assert_eq!(l.current_memory(), 0, "reservation released on failure");
}

/// A caller may abandon a successfully loaded backend before lifecycle
/// registration while a native thread still holds a clone of that backend.
/// The preflight resident budget must follow the final Arc, not the dropped
/// PreparedModel's field order.
#[tokio::test]
async fn abandoned_prepared_load_with_external_native_holder_keeps_budget() {
    let l = limits(1_000);
    let guard = l.reserve_resident(700).unwrap();
    let model: Arc<dyn Model> = Arc::new(Weights(700));
    let native_holder = Arc::clone(&model);
    let prepared = PreparedModel::for_test("pending", model, Some(guard));

    drop(prepared);
    assert_eq!(
        l.current_memory(),
        700,
        "abandoned backend still has a holder"
    );
    assert!(
        l.reserve_resident(400).is_err(),
        "abandoned prepared model recredited memory while backend is live"
    );
    drop(native_holder);

    tokio::time::timeout(Duration::from_secs(5), async {
        while l.current_memory() != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("prepared model reservation was not released on final Arc drop");
}

#[tokio::test]
async fn abandoned_prepared_load_releases_its_reservation() {
    // A caller that disconnects after backend load (cancelled IPC request)
    // drops the PreparedModel: model first, then the reservation.
    let l = limits(1_000);
    let guard = l.reserve_resident(800).unwrap();
    let prepared = PreparedModel::for_test("p", Arc::new(Weights(800)), Some(guard));
    let task = tokio::spawn(async move {
        let _held = prepared;
        std::future::pending::<()>().await;
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(l.current_memory(), 800);
    task.abort();
    let _ = task.await;
    assert_eq!(l.current_memory(), 0, "cancelled load leaks nothing");
}

#[tokio::test]
async fn registration_resizes_reservation_to_reported_footprint() {
    let (lc, limits) = setup(1_000);
    let guard = limits.reserve_resident(900).unwrap(); // file-size estimate
    let p = PreparedModel::for_test("m", Arc::new(Weights(600)), Some(guard));
    let lc = Arc::new(lc);
    register_prepared(&lc, p).await.unwrap();
    assert_eq!(limits.current_memory(), 600, "shrunk to backend report");
    lc.unload("m").await.unwrap();
    assert_eq!(limits.current_memory(), 0);
}

#[tokio::test]
async fn registration_growth_beyond_total_is_refused_and_rolled_back() {
    let (lc, limits) = setup(1_000);
    let lc = Arc::new(lc);
    let guard = limits.reserve_resident(100).unwrap();
    let p = PreparedModel::for_test("m", Arc::new(Weights(2_000)), Some(guard));
    let err = register_prepared(&lc, p).await.unwrap_err();
    assert!(is_exhausted(&err), "{err}");
    assert!(!lc.is_loaded("m").await);
    assert_eq!(limits.current_memory(), 0);
}

/// Unload must not detach resident bookkeeping before its awaited engine
/// writer is available. A cancelled old unload left a loaded model with no
/// lifecycle resident entry, even while its registry/engine handles survived.
#[tokio::test]
async fn cancelled_unload_waiting_on_engine_writer_keeps_registered_residency() {
    let (lc, limits) = setup(1_000);
    let handle = lc
        .load("cancel-unload".into(), meta("cancel-unload", 600), Arc::new(Weights(600)))
        .await
        .expect("loaded model");
    assert_eq!(limits.current_memory(), 600);

    let engine_writer = lc.engine.registration_writer().await;
    {
        let mut unloading = Box::pin(lc.unload("cancel-unload"));
        let waker = futures::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(
            std::future::Future::poll(unloading.as_mut(), &mut cx).is_pending(),
            "unload must wait for the engine writer"
        );
        // The aborted operation must not have removed its resident, registry
        // entry, engine model or index mapping while waiting for the writer.
    }
    drop(engine_writer);

    assert!(lc.registry.contains(handle).await);
    assert!(lc.engine.has_model("cancel-unload").await);
    assert!(lc.is_loaded("cancel-unload").await);
    assert!(
        lc.index.read().await.residents.contains_key("cancel-unload"),
        "cancelled unload detached resident budget bookkeeping"
    );
    assert_eq!(limits.current_memory(), 600);

    lc.unload("cancel-unload")
        .await
        .expect("retry unload after cancellation");
    assert_eq!(limits.current_memory(), 0);
    assert!(!lc.registry.contains(handle).await);
    assert!(!lc.engine.has_model("cancel-unload").await);
    assert!(!lc.is_loaded("cancel-unload").await);
}

/// Registry and engine mutation must be atomic across a cancelled load.
/// Holding only the engine write lock forces the old implementation to
/// publish a registry handle and then suspend before installing inference.
/// The corrected implementation acquires both permits before any mutation.
#[tokio::test]
async fn cancelled_registration_waiting_on_engine_lock_publishes_no_partial_model() {
    let (lc, limits) = setup(1_000);
    let engine_lock = lc.engine.registration_writer().await;
    let model: Arc<dyn Model> = Arc::new(Weights(600));
    let native_owner = Arc::clone(&model);
    let reservation = limits.reserve_resident(600).expect("preflight");

    {
        let mut pending = Box::pin(lc.load_reserved(
            "pending-engine".into(),
            meta("pending-engine", 600),
            model,
            Some(reservation),
        ));
        let waker = futures::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(
            std::future::Future::poll(pending.as_mut(), &mut cx).is_pending(),
            "registration must wait on held engine writer"
        );
        // Drop precisely where the previous code had already registered a
        // handle but not the engine model. The index + registry must roll
        // back together and the resident budget must remain with the Arc.
    }
    drop(engine_lock);

    assert_eq!(lc.registry.count().await, 0, "orphan registry handle");
    assert!(
        !lc.engine.has_model("pending-engine").await,
        "engine published cancelled model"
    );
    assert!(
        !lc.is_loaded("pending-engine").await,
        "orphan lifecycle index"
    );
    assert_eq!(
        limits.current_memory(),
        600,
        "native holder still owns bytes"
    );
    assert!(limits.reserve_resident(500).is_err());

    drop(native_owner);
    tokio::time::timeout(Duration::from_secs(5), async {
        while limits.current_memory() != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("aborted registration reservation never returned");
}

/// Cancellation while waiting for the lifecycle's index write lock must
/// not drop the preflight guard apart from the backend. Poll explicitly to
/// Pending under a held lock, then drop the future: no timing or sleep is
/// needed to establish the cancellation point.
#[tokio::test]
async fn cancelled_registration_waiting_on_index_lock_retains_model_budget() {
    let (lc, limits) = setup(1_000);
    let model: Arc<dyn Model> = Arc::new(Weights(600));
    let external_native_owner = Arc::clone(&model);
    let guard = limits.reserve_resident(600).expect("preflight reservation");
    let locked_index = lc.index.write().await;
    {
        let mut load =
            Box::pin(lc.load_reserved("pending".into(), meta("pending", 600), model, Some(guard)));
        let waker = futures::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(
            std::future::Future::poll(load.as_mut(), &mut cx).is_pending(),
            "registration must wait on held index lock"
        );
        // The aborted future must keep its resident reservation with the
        // still-live native Arc, not return it as an ordinary local variable.
    }
    drop(locked_index);

    assert!(
        !lc.is_loaded("pending").await,
        "cancelled load was registered"
    );
    assert_eq!(limits.current_memory(), 600);
    assert!(
        limits.reserve_resident(500).is_err(),
        "cancelled registration returned memory while native model was alive"
    );

    drop(external_native_owner);
    tokio::time::timeout(Duration::from_secs(5), async {
        while limits.current_memory() != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("aborted registration did not reclaim on final model drop");
}

/// A duplicate-id refusal is not an unload: a native task can retain the
/// losing prepared model after the registry refused its identity. The old
/// separate `drop(model); drop(reservation)` credited its memory early (#219).
#[tokio::test]
async fn duplicate_id_refusal_keeps_budget_until_rejected_model_holder_exits() {
    let (lc, limits) = setup(1_000);
    lc.load("m".into(), meta("m", 300), Arc::new(Weights(300)))
        .await
        .unwrap();
    let guard = limits.reserve_resident(400).unwrap();
    let model: Arc<dyn Model> = Arc::new(Weights(400));
    let native_holder = Arc::clone(&model);

    let err = lc
        .load_reserved("m".into(), meta("m", 400), model, Some(guard))
        .await
        .unwrap_err();
    assert!(matches!(err, LifecycleError::AlreadyLoaded(_)));
    assert_eq!(limits.current_memory(), 700, "rejected model still lives");
    assert!(
        limits.reserve_resident(400).is_err(),
        "second load must not overcommit while rejected backend is alive"
    );

    drop(native_holder);
    tokio::time::timeout(Duration::from_secs(5), async {
        while limits.current_memory() != 300 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("rejected model's budget was not returned on final drop");
    lc.unload("m").await.unwrap();
    assert_eq!(limits.current_memory(), 0);
}

#[tokio::test]
async fn duplicate_id_releases_the_losing_reservation() {
    let (lc, limits) = setup(1_000);
    let lc = Arc::new(lc);
    lc.load("m".into(), meta("m", 300), Arc::new(Weights(300)))
        .await
        .unwrap();
    let guard = limits.reserve_resident(300).unwrap();
    let p = PreparedModel::for_test("m", Arc::new(Weights(300)), Some(guard));
    let err = register_prepared(&lc, p).await.unwrap_err();
    assert!(matches!(
        err,
        ModelOpError::Lifecycle(LifecycleError::AlreadyLoaded(_))
    ));
    assert_eq!(
        limits.current_memory(),
        300,
        "only the winner stays charged"
    );
}

#[tokio::test]
async fn unload_defers_release_while_a_request_holds_the_model() {
    let (lc, limits) = setup(1_000);
    let model: Arc<dyn Model> = Arc::new(Weights(700));
    let in_flight = Arc::clone(&model); // a request still running on it
    lc.load("a".into(), meta("a", 700), model).await.unwrap();
    lc.unload("a").await.unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(limits.current_memory(), 700, "weights still allocated");
    let err = lc
        .load("b".into(), meta("b", 700), Arc::new(Weights(700)))
        .await
        .unwrap_err();
    assert!(matches!(err, LifecycleError::ResourceExhausted(_)), "{err}");
    drop(in_flight);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(limits.current_memory(), 0, "released once the request ends");
    lc.load("b".into(), meta("b", 700), Arc::new(Weights(700)))
        .await
        .expect("admitted after the old weights are freed");
}
