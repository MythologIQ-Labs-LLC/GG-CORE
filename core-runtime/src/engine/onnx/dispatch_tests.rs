//! Unit tests for manifest-driven ONNX loader dispatch.
//!
//! The decision function `plan_onnx_load` is pure and needs no model file or
//! the `onnx` feature; these run under default features. The not-onnx wrapper
//! tests are gated to the default build (under `--features onnx` the real
//! IO wrapper is compiled instead).

use super::{plan_onnx_load, OnnxLoadPlan};
use crate::engine::error::InferenceError;
use crate::models::manifest::{ModelArchitecture, ModelCapability, ModelManifest};

/// Build an Onnx-architecture manifest with the given capabilities + labels.
fn onnx_manifest(caps: Vec<ModelCapability>, labels: Option<Vec<String>>) -> ModelManifest {
    ModelManifest {
        model_id: "m".into(),
        name: "m".into(),
        version: "1.0.0".into(),
        capabilities: caps,
        sha256: "0".repeat(64),
        size_bytes: 0,
        architecture: ModelArchitecture::Onnx,
        license: "MIT".into(),
        labels,
    }
}

fn err_message<T>(result: Result<T, InferenceError>) -> String {
    match result {
        Err(InferenceError::ModelError(msg)) => msg,
        Err(_) => panic!("expected ModelError variant"),
        Ok(_) => panic!("expected Err, got Ok"),
    }
}

#[test]
fn plan_classifier_with_labels_returns_ordered_labels() {
    let m = onnx_manifest(
        vec![ModelCapability::TextClassification],
        Some(vec!["a".into(), "b".into()]),
    );
    assert_eq!(
        plan_onnx_load(&m).unwrap(),
        OnnxLoadPlan::Classifier(vec!["a".into(), "b".into()])
    );
}

#[test]
fn plan_classifier_without_labels_is_error() {
    let m = onnx_manifest(vec![ModelCapability::TextClassification], None);
    assert!(err_message(plan_onnx_load(&m)).contains("labels"));
}

#[test]
fn plan_classifier_empty_labels_is_error() {
    let m = onnx_manifest(vec![ModelCapability::TextClassification], Some(vec![]));
    assert!(err_message(plan_onnx_load(&m)).contains("labels"));
}

#[test]
fn plan_embedding_returns_embedder() {
    let m = onnx_manifest(vec![ModelCapability::Embedding], None);
    assert_eq!(plan_onnx_load(&m).unwrap(), OnnxLoadPlan::Embedder);
}

#[test]
fn plan_classification_plus_ner_resolves_to_classifier() {
    let m = onnx_manifest(
        vec![
            ModelCapability::TextClassification,
            ModelCapability::NamedEntityRecognition,
        ],
        Some(vec!["x".into()]),
    );
    assert_eq!(
        plan_onnx_load(&m).unwrap(),
        OnnxLoadPlan::Classifier(vec!["x".into()])
    );
}

#[test]
fn plan_both_servable_is_ambiguous_error() {
    let m = onnx_manifest(
        vec![
            ModelCapability::TextClassification,
            ModelCapability::Embedding,
        ],
        Some(vec!["x".into()]),
    );
    assert!(err_message(plan_onnx_load(&m)).contains("ambiguous"));
}

#[test]
fn plan_non_onnx_architecture_is_error() {
    let mut m = onnx_manifest(vec![ModelCapability::Embedding], None);
    m.architecture = ModelArchitecture::Gguf;
    assert!(err_message(plan_onnx_load(&m)).contains("Gguf"));
}

#[test]
fn plan_no_servable_capability_is_error() {
    let m = onnx_manifest(vec![ModelCapability::TextGeneration], None);
    assert!(err_message(plan_onnx_load(&m)).contains("no ONNX-servable capability"));
}

#[cfg(not(feature = "onnx"))]
#[test]
fn load_from_manifest_stub_rejects_bad_manifest_before_feature_error() {
    use super::load_onnx_from_manifest;
    use crate::engine::onnx::OnnxConfig;
    use std::path::Path;

    let mut m = onnx_manifest(vec![ModelCapability::Embedding], None);
    m.architecture = ModelArchitecture::Gguf;
    let err = load_onnx_from_manifest(&m, Path::new("x.onnx"), &OnnxConfig::default());
    assert!(err_message(err).contains("Gguf"));
}

#[cfg(not(feature = "onnx"))]
#[test]
fn load_from_manifest_stub_valid_manifest_reports_feature_absent() {
    use super::load_onnx_from_manifest;
    use crate::engine::onnx::OnnxConfig;
    use std::path::Path;

    let m = onnx_manifest(vec![ModelCapability::Embedding], None);
    let err = load_onnx_from_manifest(&m, Path::new("x.onnx"), &OnnxConfig::default());
    assert!(err_message(err).contains("not compiled in"));
}
