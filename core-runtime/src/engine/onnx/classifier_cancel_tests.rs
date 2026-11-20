//! Regression tests for the ONNX classifier's cooperative stop contract.
//! Classification has one native evaluation, so cancellation is checked before
//! evaluation and before publishing its result. A running evaluation cannot
//! be preempted by this callback.

use super::*;
use crate::engine::Model;
use std::sync::atomic::{AtomicUsize, Ordering};

fn unloaded_classifier() -> OnnxClassifier {
    OnnxClassifier::new(
        "not-loaded".into(),
        vec!["negative".into(), "positive".into()],
    )
}

#[tokio::test]
async fn pre_cancelled_classification_does_not_access_unloaded_model() {
    let classifier = unloaded_classifier();
    let polls = AtomicUsize::new(0);
    let cancelled = || {
        polls.fetch_add(1, Ordering::SeqCst);
        true
    };
    let result = classifier
        .infer_cancellable(
            &InferenceInput::Text("hello".into()),
            &InferenceConfig::for_classification(),
            Some(&cancelled),
        )
        .await;
    assert!(
        matches!(&result, Err(InferenceError::ModelError(message)) if message == "ONNX classification cancelled"),
        "{result:?}"
    );
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn non_cancelled_classifier_preserves_original_model_error() {
    let classifier = unloaded_classifier();
    let polls = AtomicUsize::new(0);
    let never_cancel = || {
        polls.fetch_add(1, Ordering::SeqCst);
        false
    };
    let result = classifier
        .infer_cancellable(
            &InferenceInput::Text("hello".into()),
            &InferenceConfig::for_classification(),
            Some(&never_cancel),
        )
        .await;
    assert!(
        matches!(&result, Err(InferenceError::ModelError(message)) if message.contains("not loaded")),
        "{result:?}"
    );
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_does_not_turn_unsupported_classification_batch_into_success() {
    let classifier = unloaded_classifier();
    let never_cancel = || false;
    let result = classifier
        .infer_cancellable(
            &InferenceInput::TextBatch(vec!["one".into(), "two".into()]),
            &InferenceConfig::for_classification(),
            Some(&never_cancel),
        )
        .await;
    assert!(
        matches!(&result, Err(InferenceError::CapabilityNotSupported(message)) if message.contains("batch")),
        "{result:?}"
    );
}
