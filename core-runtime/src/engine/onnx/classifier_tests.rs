//! Classifier tests. The `logits_to_classification` tests are pure (synthetic
//! tensors, no model) and run in CI under the `onnx` feature; `load_and_classify`
//! is fixture-gated and skips when the (gitignored) model is absent.

use super::*;
use candle_core::{Device, Tensor};

#[test]
fn logits_to_classification_picks_argmax() {
    let logits = Tensor::new(vec![-1.0f32, 3.0, 0.5], &Device::Cpu).unwrap();
    let labels = vec!["neg".to_string(), "pos".to_string(), "neu".to_string()];

    let r = logits_to_classification(&logits, &labels).expect("classify");

    assert_eq!(r.label, "pos", "argmax label");
    assert_eq!(r.all_labels.len(), 3);
    assert_eq!(r.all_labels[0].0, "pos", "top of sorted list");
    // Sorted descending by confidence.
    assert!(r.all_labels[0].1 >= r.all_labels[1].1);
    assert!(r.all_labels[1].1 >= r.all_labels[2].1);
    // Softmax probabilities sum to ~1.0.
    let sum: f32 = r.all_labels.iter().map(|(_, p)| *p).sum();
    assert!((sum - 1.0).abs() < 1e-4, "probs sum = {sum}");
    // Reported confidence is the top probability.
    assert!((r.confidence - r.all_labels[0].1).abs() < 1e-6);
}

#[test]
fn classifier_accepts_single_sample_rank_one_or_two_logits() {
    let labels = vec!["negative".to_string(), "positive".to_string()];
    let flat = Tensor::from_vec(vec![-1.0_f32, 3.0], 2, &Device::Cpu).unwrap();
    let batched = Tensor::from_vec(vec![-1.0_f32, 3.0], (1, 2), &Device::Cpu).unwrap();
    for logits in [flat, batched] {
        let result = logits_to_classification(&logits, &labels).expect("single sample logits");
        assert_eq!(result.label, "positive");
        assert!(result.confidence.is_finite());
    }
}

#[test]
fn classifier_refuses_multi_row_and_extra_rank_logits() {
    // Old flatten_all accepted [2,1] with two labels as if two independent
    // samples were competing classes; [1,1,2] was accepted too.
    let labels = vec!["negative".to_string(), "positive".to_string()];
    let ambiguous = [
        Tensor::from_vec(vec![-1.0_f32, 3.0], (2, 1), &Device::Cpu).unwrap(),
        Tensor::from_vec(vec![-1.0_f32, 3.0], (1, 1, 2), &Device::Cpu).unwrap(),
        Tensor::from_vec(vec![-1.0_f32, 3.0], (1, 2, 1), &Device::Cpu).unwrap(),
    ];
    for logits in ambiguous {
        let result = logits_to_classification(&logits, &labels);
        assert!(
            matches!(result, Err(InferenceError::ModelError(message)) if message.contains("shape")),
            "non-scalar classification must fail closed"
        );
    }

    // Even if the total element count coincides with a larger label set,
    // a true [batch, classes] matrix can never represent one classification.
    let four_labels = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    let multi_row = Tensor::from_vec(vec![0.1_f32, 0.2, 0.3, 0.4], (2, 2), &Device::Cpu)
        .expect("construct batch logits");
    assert!(logits_to_classification(&multi_row, &four_labels).is_err());
}

#[test]
fn logits_to_classification_rejects_label_mismatch() {
    let logits = Tensor::new(vec![0.1f32, 0.2, 0.3], &Device::Cpu).unwrap();
    let labels = vec!["a".to_string(), "b".to_string()]; // 2 labels vs 3 logits
    assert!(logits_to_classification(&logits, &labels).is_err());
}

#[test]
fn classify_text_without_model_fails() {
    // No model attached -> fail loud (not mock output).
    let clf = OnnxClassifier::new("c".into(), vec!["a".into(), "b".into()]);
    assert!(clf.classify_text("hello").is_err());
}

#[test]
fn load_and_classify() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/models/onnx/tinybert-classifier.onnx");
    if !path.exists() {
        eprintln!(
            "skipping load_and_classify: fixture {} not present",
            path.display()
        );
        return;
    }
    // Skip when the fixture is absent OR a placeholder/invalid ONNX file
    // (fixtures/ is gitignored; local copies may be LFS pointers or stubs).
    let model = match candle_onnx::read_file(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("skipping load_and_classify: fixture is not a valid ONNX model ({e})");
            return;
        }
    };
    let clf = OnnxClassifier::with_model(
        "tinybert".into(),
        vec!["negative".into(), "positive".into()],
        model,
        super::super::tokenizer::OnnxTokenizer::for_model(&path),
    );
    // The fixture's exact label count is not assumed here; a well-formed result
    // OR a clean label-mismatch error both prove the inference path runs
    // end-to-end (load -> simple_eval -> logits extraction) without panicking.
    match clf.classify_text("this is great") {
        Ok(r) => {
            assert!((0.0..=1.0).contains(&r.confidence));
            assert!(!r.all_labels.is_empty());
        }
        Err(InferenceError::ModelError(_)) => {}
        Err(e) => panic!("unexpected error: {e:?}"),
    }
}

#[test]
fn nonfinite_model_logits_are_refused_before_softmax() {
    let labels = vec!["negative".to_string(), "positive".to_string()];
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let logits = Tensor::new(vec![invalid, 1.0], &Device::Cpu).expect("construct logits");
        let error =
            logits_to_classification(&logits, &labels).expect_err("invalid logits must fail");
        assert!(
            matches!(error, InferenceError::ModelError(message) if message.contains("non-finite")),
            "non-finite logits must not yield success-shaped confidences"
        );
    }
}

#[tokio::test]
async fn embedded_classifier_rejects_all_batch_sizes_instead_of_dropping_items() {
    use crate::engine::Model as _;

    let classifier = OnnxClassifier::new(
        "classifier".into(),
        vec!["negative".into(), "positive".into()],
    );
    for batch in [
        vec!["one".to_string()],
        vec!["one".to_string(), "two".to_string()],
    ] {
        let result = classifier
            .infer(
                &InferenceInput::TextBatch(batch),
                &InferenceConfig::for_classification(),
            )
            .await;
        assert!(
            matches!(result, Err(InferenceError::CapabilityNotSupported(message)) if message.contains("batch")),
            "even a singleton batch must not return a scalar classification"
        );
    }
}

fn classifier_with_initializer_bytes(bytes: usize) -> OnnxClassifier {
    // This synthetic ONNX is not intended for inference; it proves the exact
    // resident bytes reported from graph initializer storage, rather than
    // depending on an external or unlicensed model download.
    let initializer = candle_onnx::onnx::TensorProto {
        raw_data: vec![0u8; bytes],
        ..Default::default()
    };
    let graph = candle_onnx::onnx::GraphProto {
        initializer: vec![initializer],
        ..Default::default()
    };
    let model = candle_onnx::onnx::ModelProto {
        graph: Some(graph),
        ..Default::default()
    };
    OnnxClassifier::with_model(
        "accounting-classifier".into(),
        vec!["NEGATIVE".into(), "POSITIVE".into()],
        model,
        super::super::tokenizer::OnnxTokenizer::HashFallback,
    )
}

#[tokio::test]
async fn loaded_classifier_reports_weight_bytes_and_clears_after_unload() {
    use crate::engine::Model as _;
    let mut classifier = classifier_with_initializer_bytes(4_096);
    assert_eq!(
        classifier.memory_usage(),
        4_096,
        "loaded classifier must never report zero for nonempty initializer"
    );
    classifier.unload().await.unwrap();
    assert_eq!(classifier.memory_usage(), 0);
}

#[tokio::test]
async fn classifier_memory_reservation_shares_budget_with_working_requests() {
    use crate::engine::InferenceEngine;
    use crate::engine::Model;
    use crate::memory::{ResourceLimits, ResourceLimitsConfig};
    use crate::models::{ModelLifecycle, ModelMetadata, ModelRegistry};
    use std::sync::Arc;

    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 800,
        max_total_memory: 6_000,
        max_concurrent: 2,
    });
    let engine = Arc::new(InferenceEngine::new(4_096));
    let registry = Arc::new(ModelRegistry::new());
    let lifecycle = ModelLifecycle::new(registry, engine).with_resource_limits(limits.clone());
    let classifier: Arc<dyn Model> = Arc::new(classifier_with_initializer_bytes(5_000));
    lifecycle
        .load(
            "accounting-classifier".into(),
            ModelMetadata {
                name: "accounting-classifier".into(),
                size_bytes: 5_000,
            },
            classifier,
        )
        .await
        .expect("classifier must register when its weights fit");

    assert_eq!(limits.current_memory(), 5_000);
    let active = limits
        .try_acquire(800)
        .expect("classifier weights and one working reservation fit");
    assert_eq!(limits.current_memory(), 5_800);
    assert!(
        limits.try_acquire(300).is_err(),
        "second working reservation must not exceed total-memory cap"
    );
    assert_eq!(limits.current_memory(), 5_800);
    drop(active);
    lifecycle.unload("accounting-classifier").await.unwrap();
    assert_eq!(limits.current_memory(), 0);
}

#[tokio::test]
async fn oversized_classifier_is_refused_before_registration_without_leak() {
    use crate::engine::InferenceEngine;
    use crate::engine::Model;
    use crate::memory::{ResourceLimits, ResourceLimitsConfig};
    use crate::models::{ModelLifecycle, ModelMetadata, ModelRegistry};
    use std::sync::Arc;

    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 500,
        max_total_memory: 1_000,
        max_concurrent: 2,
    });
    let lifecycle = ModelLifecycle::new(
        Arc::new(ModelRegistry::new()),
        Arc::new(InferenceEngine::new(4_096)),
    )
    .with_resource_limits(limits.clone());
    let model: Arc<dyn Model> = Arc::new(classifier_with_initializer_bytes(1_001));
    let result = lifecycle
        .load(
            "too-large-classifier".into(),
            ModelMetadata {
                name: "too-large-classifier".into(),
                size_bytes: 1_001,
            },
            model,
        )
        .await;
    assert!(matches!(
        result,
        Err(crate::models::LifecycleError::ResourceExhausted(_))
    ));
    assert!(!lifecycle.is_loaded("too-large-classifier").await);
    assert_eq!(limits.current_memory(), 0);
}
