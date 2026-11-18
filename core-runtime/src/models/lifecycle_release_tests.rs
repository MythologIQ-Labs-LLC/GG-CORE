//! Verified release of unloaded instances (MODEL_RELEASE_V1_PLAN A1–A7, R1).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::resident_tests::{meta, setup, Weights};
use super::*;
use crate::engine::Model;
use crate::models::registry::LoadedModelState;

/// Load `id` and keep a clone of the model, standing in for an in-flight
/// request that still holds it.
async fn load_held(lc: &ModelLifecycle, id: &str, bytes: usize) -> Arc<dyn Model> {
    let model: Arc<dyn Model> = Arc::new(Weights(bytes));
    lc.load(id.into(), meta(id, bytes as u64), Arc::clone(&model))
        .await
        .expect("load");
    model
}

#[tokio::test]
async fn idle_unload_is_released_at_once_and_budget_returns() {
    let (lc, limits) = setup(1_000);
    drop(load_held(&lc, "a", 600).await);
    lc.unload("a").await.unwrap();
    assert_eq!(lc.wait_released("a", Duration::ZERO).await, (true, 0));
    assert!(lc.releasing_models().is_empty());
    assert_eq!(limits.current_memory(), 0);
}

#[tokio::test]
async fn held_instance_is_listed_and_charged_until_the_holder_drops() {
    let (lc, limits) = setup(1_000);
    let holder = load_held(&lc, "a", 600).await;
    let handle = lc.get_handle("a").await.unwrap();
    lc.unload("a").await.unwrap();

    let listed = lc.releasing_models();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle_id, handle.id(), "original handle kept");
    assert_eq!(listed[0].name, "a");
    assert_eq!(listed[0].state, LoadedModelState::Unloading);
    assert_eq!(limits.current_memory(), 600, "still charged while held");
    assert_eq!(lc.wait_released("a", Duration::ZERO).await, (false, 1));

    drop(holder);
    assert_eq!(
        lc.wait_released("a", Duration::from_secs(5)).await,
        (true, 0)
    );
    assert_eq!(limits.current_memory(), 0, "budget returned after release");
    assert!(lc.releasing_models().is_empty());
}

#[tokio::test]
async fn a_short_wait_returns_false_without_blocking_past_it() {
    let (lc, _limits) = setup(1_000);
    let _holder = load_held(&lc, "a", 600).await;
    lc.unload("a").await.unwrap();
    let start = Instant::now();
    let outcome = lc.wait_released("a", Duration::from_millis(200)).await;
    assert_eq!(outcome, (false, 1));
    assert!(
        start.elapsed() < Duration::from_millis(300),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn waiting_holds_no_lifecycle_lock() {
    let (lc, _limits) = setup(1_000);
    let lc = Arc::new(lc);
    let _holder = load_held(&lc, "a", 300).await;
    lc.unload("a").await.unwrap();
    let waiter = {
        let lc = Arc::clone(&lc);
        tokio::spawn(async move { lc.wait_released("a", Duration::from_secs(5)).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let other = tokio::time::timeout(Duration::from_secs(1), load_held(&lc, "b", 100)).await;
    assert!(other.is_ok(), "load blocked by a pending release wait");
    assert!(!waiter.is_finished(), "the holder is still alive");
}

#[tokio::test]
async fn a_reloaded_id_and_its_old_instance_are_tracked_apart() {
    let (lc, _limits) = setup(1_000);
    let old = load_held(&lc, "a", 300).await;
    lc.unload("a").await.unwrap();
    let new = load_held(&lc, "a", 300).await;
    lc.unload("a").await.unwrap();
    assert_eq!(lc.wait_released("a", Duration::ZERO).await, (false, 2));
    drop(old);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(lc.wait_released("a", Duration::ZERO).await, (false, 1));
    drop(new);
    assert_eq!(
        lc.wait_released("a", Duration::from_secs(5)).await,
        (true, 0)
    );
}

#[tokio::test]
async fn unlimited_lifecycles_track_release_too() {
    let registry = Arc::new(ModelRegistry::new());
    let engine = Arc::new(InferenceEngine::new(4096));
    let lc = ModelLifecycle::new(registry, engine);
    let holder = load_held(&lc, "a", 600).await;
    lc.unload("a").await.unwrap();
    assert_eq!(lc.wait_released("a", Duration::ZERO).await, (false, 1));
    drop(holder);
    assert_eq!(
        lc.wait_released("a", Duration::from_secs(5)).await,
        (true, 0)
    );
}

/// The supervisor does not depend on Tokio being active at the unload call.
/// It keeps the budget charged while the native holder is live, then returns
/// capacity after the actual final model reference is released.
#[test]
fn without_a_runtime_release_reclaims_only_after_last_holder_drops() {
    let (lc, limits) = setup(1_000);
    let holder = futures::executor::block_on(load_held(&lc, "a", 600));
    futures::executor::block_on(lc.unload("a")).unwrap();
    assert_eq!(lc.releasing_models().len(), 1);
    assert_eq!(limits.current_memory(), 600);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        limits.current_memory(),
        600,
        "native holder still owns model"
    );

    drop(holder);
    let start = Instant::now();
    while !lc.releasing_models().is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        lc.releasing_models().is_empty(),
        "release supervisor did not drain"
    );
    assert_eq!(limits.current_memory(), 0, "model release returned budget");
}

/// Dropping a Tokio runtime aborts its async tasks but must not abort model
/// resident ownership while a detached native inference thread still holds
/// the model. The background supervisor is intentionally runtime-independent.
#[test]
fn runtime_shutdown_preserves_resident_budget_until_model_is_released() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (lc, limits) = setup(1_000);
    let holder = rt.block_on(load_held(&lc, "a", 600));
    rt.block_on(lc.unload("a")).unwrap();
    assert_eq!(limits.current_memory(), 600);
    drop(rt);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        limits.current_memory(),
        600,
        "runtime shutdown freed live bytes"
    );
    let tracked = lc.releasing_models();
    assert_eq!(tracked.len(), 1);
    assert_eq!(tracked[0].state, LoadedModelState::Unloading);

    drop(holder);
    let start = Instant::now();
    while !lc.releasing_models().is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        lc.releasing_models().is_empty(),
        "release failed after runtime drop"
    );
    assert_eq!(limits.current_memory(), 0);
}

/// Even dropping the lifecycle owner must not free a resident budget while
/// detached native work retains the last external model reference.
#[test]
fn lifecycle_and_runtime_teardown_keep_live_model_memory_charged() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (lc, limits) = setup(1_000);
    let holder = rt.block_on(load_held(&lc, "a", 600));
    rt.block_on(lc.unload("a")).unwrap();
    drop(lc);
    drop(rt);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        limits.current_memory(),
        600,
        "lifecycle teardown falsely credited resident memory"
    );
    drop(holder);
    let start = Instant::now();
    while limits.current_memory() != 0 && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        limits.current_memory(),
        0,
        "last model holder was not reclaimed"
    );
}

/// R1: an unload racing listings never makes a held instance disappear or
/// appear twice (registry read first, tracker second, dedupe by handle).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_instance_is_listed_exactly_once_while_unloading() {
    let (lc, _limits) = setup(1_000);
    let lc = Arc::new(lc);
    let _holder = load_held(&lc, "a", 300).await;
    let unloader = {
        let lc = Arc::clone(&lc);
        tokio::spawn(async move { lc.unload("a").await })
    };
    for _ in 0..1_000 {
        let mut seen = lc.registry.list_models().await;
        let registry_ids: Vec<u64> = seen.iter().map(|m| m.handle_id).collect();
        for info in lc.releasing_models() {
            if !registry_ids.contains(&info.handle_id) {
                seen.push(info);
            }
        }
        assert_eq!(seen.len(), 1, "held instance listed {} times", seen.len());
    }
    unloader.await.unwrap().unwrap();
}
