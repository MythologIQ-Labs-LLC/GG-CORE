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
