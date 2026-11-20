//! Native ONNX fixture regressions for cooperative embedding cancellation.
//! The trait previously ignored its callback, executing the entire batch.

use super::*;
use crate::engine::Model;
use std::sync::atomic::{AtomicUsize, Ordering};

const DIM: usize = 8;

fn fixture() -> OnnxEmbedder {
    let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/models/onnx");
    OnnxEmbedder::load(&models, "tiny-embedder", DIM).expect("committed real graph must load")
}

#[tokio::test]
async fn pre_cancelled_single_does_not_access_unloaded_model() {
    let model = OnnxEmbedder::new("not-loaded".into(), DIM);
    let cancelled = || true;
    let result = model
        .infer_cancellable(
            &InferenceInput::Text("hello".into()),
            &InferenceConfig::default(),
            Some(&cancelled),
        )
        .await;
    assert!(matches!(result, Err(InferenceError::Cancelled)), "{result:?}");
}

#[tokio::test]
async fn cancellation_after_single_eval_refuses_success_result() {
    let model = fixture();
    let polls = AtomicUsize::new(0);
    let stop = || polls.fetch_add(1, Ordering::SeqCst) >= 1;
    let result = model
        .infer_cancellable(
            &InferenceInput::Text("hello world".into()),
            &InferenceConfig::default(),
            Some(&stop),
        )
        .await;
    assert!(matches!(result, Err(InferenceError::Cancelled)), "{result:?}");
    assert_eq!(polls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancellation_after_first_batch_item_stops_remaining_items() {
    let model = fixture();
    let polls = AtomicUsize::new(0);
    // First poll is before dispatch, second before item one, third after
    // item one. A backend that ignores this callback wrongly returns all.
    let stop = || polls.fetch_add(1, Ordering::SeqCst) >= 2;
    let result = model
        .infer_cancellable(
            &InferenceInput::TextBatch(vec![
                "hello world".into(),
                "greatest good".into(),
                "hello".into(),
            ]),
            &InferenceConfig::default(),
            Some(&stop),
        )
        .await;
    assert!(matches!(result, Err(InferenceError::Cancelled)), "{result:?}");
    assert_eq!(polls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn never_cancelled_callback_preserves_real_batch_embeddings() {
    let model = fixture();
    let stop = || false;
    let result = model
        .infer_cancellable(
            &InferenceInput::TextBatch(vec!["hello".into(), "world".into()]),
            &InferenceConfig::default(),
            Some(&stop),
        )
        .await;
    let Ok(InferenceOutput::EmbeddingBatch(items)) = result else {
        panic!("valid ONNX batch must complete with embeddings");
    };
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| item.dimensions == DIM));
}
