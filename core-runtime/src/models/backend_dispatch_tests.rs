//! Tests for manifest-driven backend dispatch.
//!
//! `choose_backend` is pure (constructed manifests, no files). The
//! `load_model_dispatch` routing tests run under default features and assert
//! the *selected* loader via its feature-gated error ("ONNX/GGUF support not
//! compiled in") — proving which backend a manifest routed to.

use super::{choose_backend, load_model_dispatch, BackendChoice};
use crate::engine::InferenceError;
use crate::models::manifest::{ModelArchitecture, ModelCapability, ModelManifest};

fn manifest(arch: ModelArchitecture, caps: Vec<ModelCapability>) -> ModelManifest {
    ModelManifest {
        model_id: "m".into(),
        name: "m".into(),
        version: "1.0.0".into(),
        capabilities: caps,
        sha256: "0".repeat(64),
        size_bytes: 0,
        architecture: arch,
        license: "MIT".into(),
        labels: None,
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
fn choose_onnx_manifest_selects_onnx() {
    let m = manifest(ModelArchitecture::Onnx, vec![ModelCapability::Embedding]);
    match choose_backend(Some(m)) {
        BackendChoice::Onnx(carried) => {
            assert_eq!(carried.architecture, ModelArchitecture::Onnx);
        }
        BackendChoice::GgufDefault => panic!("expected Onnx"),
    }
}

#[test]
fn choose_gguf_manifest_selects_gguf_default() {
    let m = manifest(
        ModelArchitecture::Gguf,
        vec![ModelCapability::TextGeneration],
    );
    assert!(matches!(
        choose_backend(Some(m)),
        BackendChoice::GgufDefault
    ));
}

#[test]
fn choose_safetensors_manifest_selects_gguf_default() {
    let m = manifest(
        ModelArchitecture::SafeTensors,
        vec![ModelCapability::Embedding],
    );
    assert!(matches!(
        choose_backend(Some(m)),
        BackendChoice::GgufDefault
    ));
}

#[test]
fn choose_absent_manifest_selects_gguf_default() {
    assert!(matches!(choose_backend(None), BackendChoice::GgufDefault));
}

#[cfg(not(feature = "onnx"))]
#[test]
fn dispatch_routes_onnx_manifest_to_onnx_loader() {
    let dir = std::env::temp_dir().join("gg_core_b29b2_onnx_route");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_json = r#"{
        "model_id": "m", "name": "m", "version": "1.0.0",
        "capabilities": ["embedding"],
        "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
        "size_bytes": 0, "architecture": "onnx", "license": "MIT"
    }"#;
    std::fs::write(dir.join("manifest.json"), manifest_json).unwrap();

    let err = load_model_dispatch(&dir.join("model.onnx"), "m");
    // Default features: the ONNX loader stub proves it routed to ONNX.
    assert!(err_message(err).contains("ONNX support not compiled in"));
}

#[cfg(not(feature = "gguf"))]
#[test]
fn dispatch_routes_no_manifest_to_gguf_loader() {
    let dir = std::env::temp_dir().join("gg_core_b29b2_gguf_route");
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join("manifest.json")); // ensure absent

    let err = load_model_dispatch(&dir.join("model.gguf"), "m");
    // No manifest → GGUF default; the GGUF loader stub proves the route.
    assert!(err_message(err).contains("GGUF support not compiled in"));
}
