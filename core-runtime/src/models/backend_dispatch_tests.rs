//! Tests for manifest-driven backend dispatch.
//!
//! `choose_backend` is pure (constructed manifests, no files). The
//! `load_model_dispatch` routing tests run under default features and assert
//! the *selected* loader via its feature-gated error ("ONNX/GGUF support not
//! compiled in") — proving which backend a manifest routed to.

use super::{choose_backend, load_model_dispatch, read_sibling_manifest, BackendChoice};
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
        other => panic!("expected Onnx, got {other:?}"),
    }
}

#[test]
fn choose_gguf_manifest_is_explicit_not_a_fallback() {
    let m = manifest(
        ModelArchitecture::Gguf,
        vec![ModelCapability::TextGeneration],
    );
    assert!(matches!(
        choose_backend(Some(m)),
        BackendChoice::GgufManifest(_)
    ));
}

#[test]
fn unsupported_safetensors_manifest_does_not_fall_back_to_gguf() {
    let m = manifest(
        ModelArchitecture::SafeTensors,
        vec![ModelCapability::Embedding],
    );
    assert!(matches!(
        choose_backend(Some(m)),
        BackendChoice::Unsupported(ModelArchitecture::SafeTensors)
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
    let model_path = dir.join("model.onnx");
    std::fs::write(&model_path, b"onnx bytes").unwrap();
    // Integrity enforcement verifies a parsed manifest's sha256 before
    // dispatch, so declare the file's real hash.
    let sha = crate::models::integrity::file_sha256_hex(&model_path).unwrap();
    let manifest_json = format!(
        r#"{{
        "model_id": "m", "name": "m", "version": "1.0.0",
        "capabilities": ["embedding"],
        "sha256": "{sha}",
        "size_bytes": 10, "architecture": "onnx", "license": "MIT"
    }}"#
    );
    std::fs::write(dir.join("manifest.json"), manifest_json).unwrap();

    let err = load_model_dispatch(&model_path, "m");
    // Default features: the ONNX loader stub proves it routed to ONNX.
    assert!(err_message(err).contains("ONNX support not compiled in"));
}

fn fresh_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gg_core_manifest_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A present but malformed manifest must fail the load, never degrade to
/// a manifest-free (unverified) load.
#[test]
fn present_but_malformed_manifest_fails_closed() {
    let dir = fresh_dir("malformed");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"gguf bytes").unwrap();
    std::fs::write(dir.join("manifest.json"), b"{ this is not json").unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("invalid manifest JSON"), "got: {msg}");
}

/// A manifest whose integrity field is not a usable digest is invalid.
#[test]
fn manifest_with_unusable_sha256_fails_closed() {
    let dir = fresh_dir("badsha");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"gguf bytes").unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        br#"{"model_id":"m","name":"m","version":"1.0.0","capabilities":["text_generation"],
            "sha256":"abc","size_bytes":10,"architecture":"gguf","license":"MIT"}"#,
    )
    .unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("sha256 must be 64 hex"), "got: {msg}");
}

/// Absent manifest is the legitimate manifest-free GGUF case.
#[test]
fn absent_manifest_reads_as_none() {
    let dir = fresh_dir("absent");
    let model_path = dir.join("model.gguf");
    assert!(read_sibling_manifest(&model_path).unwrap().is_none());
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

#[test]
fn manifest_free_onnx_requires_explicit_manifest() {
    let dir = fresh_dir("onnx_no_manifest");
    let model_path = dir.join("model.onnx");
    std::fs::write(&model_path, b"irrelevant onnx input").unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("requires a validated sibling manifest"), "got: {msg}");
}

#[test]
fn manifest_free_safetensors_never_falls_back_to_gguf() {
    let dir = fresh_dir("safetensors_no_manifest");
    let model_path = dir.join("model.safetensors");
    std::fs::write(&model_path, b"irrelevant safetensors input").unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("requires a validated sibling manifest"), "got: {msg}");
}

#[test]
fn explicit_safetensors_manifest_is_rejected_before_backend_work() {
    let dir = fresh_dir("safetensors_explicit");
    let model_path = dir.join("model.safetensors");
    std::fs::write(&model_path, b"irrelevant safetensors input").unwrap();
    let mut m = manifest(ModelArchitecture::SafeTensors, vec![ModelCapability::Embedding]);
    m.sha256 = crate::models::integrity::file_sha256_hex(&model_path).unwrap();
    m.size_bytes = std::fs::metadata(&model_path).unwrap().len();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&m).unwrap(),
    ).unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("unsupported model backend: SafeTensors"), "got: {msg}");
}

#[test]
fn gguf_embedding_only_manifest_is_not_accepted_as_text_generation() {
    let dir = fresh_dir("gguf_claims_embedding");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"irrelevant gguf input").unwrap();
    let mut m = manifest(ModelArchitecture::Gguf, vec![ModelCapability::Embedding]);
    m.sha256 = crate::models::integrity::file_sha256_hex(&model_path).unwrap();
    m.size_bytes = std::fs::metadata(&model_path).unwrap().len();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&m).unwrap(),
    ).unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("unsupported GGUF capabilities"), "got: {msg}");
}

#[test]
fn gguf_declared_generation_plus_embedding_is_not_silently_accepted() {
    let dir = fresh_dir("gguf_claims_two");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"irrelevant gguf input").unwrap();
    let mut m = manifest(
        ModelArchitecture::Gguf,
        vec![ModelCapability::TextGeneration, ModelCapability::Embedding],
    );
    m.sha256 = crate::models::integrity::file_sha256_hex(&model_path).unwrap();
    m.size_bytes = std::fs::metadata(&model_path).unwrap().len();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&m).unwrap(),
    ).unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("unsupported GGUF capabilities"), "got: {msg}");
}

#[test]
fn onnx_generation_is_not_advertised_as_serving_supported() {
    let dir = fresh_dir("onnx_generation");
    let model_path = dir.join("model.onnx");
    std::fs::write(&model_path, b"irrelevant onnx input").unwrap();
    let mut m = manifest(ModelArchitecture::Onnx, vec![ModelCapability::TextGeneration]);
    m.sha256 = crate::models::integrity::file_sha256_hex(&model_path).unwrap();
    m.size_bytes = std::fs::metadata(&model_path).unwrap().len();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&m).unwrap(),
    ).unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("no ONNX-servable capability"), "got: {msg}");
}

#[cfg(not(feature = "gguf"))]
#[test]
fn verified_gguf_text_generation_manifest_still_reaches_backend() {
    let dir = fresh_dir("gguf_generation_valid");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"irrelevant gguf input").unwrap();
    let mut m = manifest(ModelArchitecture::Gguf, vec![ModelCapability::TextGeneration]);
    m.sha256 = crate::models::integrity::file_sha256_hex(&model_path).unwrap();
    m.size_bytes = std::fs::metadata(&model_path).unwrap().len();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&m).unwrap(),
    ).unwrap();
    let msg = err_message(load_model_dispatch(&model_path, "m"));
    assert!(msg.contains("GGUF support not compiled in"), "got: {msg}");
}


#[test]
fn two_models_in_one_directory_use_independent_pinned_manifests() {
    let dir = fresh_dir("two_independent_manifests");
    let gguf_file = dir.join("qwen.gguf");
    let onnx_file = dir.join("embedder.onnx");
    std::fs::write(&gguf_file, b"model A bytes").unwrap();
    std::fs::write(&onnx_file, b"model B bytes").unwrap();

    let mut gguf = manifest(ModelArchitecture::Gguf, vec![ModelCapability::TextGeneration]);
    gguf.model_id = "qwen".into();
    gguf.sha256 = crate::models::integrity::file_sha256_hex(&gguf_file).unwrap();
    gguf.size_bytes = std::fs::metadata(&gguf_file).unwrap().len();
    let mut onnx = manifest(ModelArchitecture::Onnx, vec![ModelCapability::Embedding]);
    onnx.model_id = "embedder".into();
    onnx.sha256 = crate::models::integrity::file_sha256_hex(&onnx_file).unwrap();
    onnx.size_bytes = std::fs::metadata(&onnx_file).unwrap().len();

    std::fs::write(
        dir.join("qwen.gguf.manifest.json"),
        serde_json::to_string(&gguf).unwrap(),
    ).unwrap();
    std::fs::write(
        dir.join("embedder.onnx.manifest.json"),
        serde_json::to_string(&onnx).unwrap(),
    ).unwrap();
    let got_gguf = read_sibling_manifest(&gguf_file).unwrap().unwrap();
    let got_onnx = read_sibling_manifest(&onnx_file).unwrap().unwrap();
    assert_eq!(got_gguf.model_id, "qwen");
    assert_eq!(got_gguf.architecture, ModelArchitecture::Gguf);
    assert_eq!(got_onnx.model_id, "embedder");
    assert_eq!(got_onnx.architecture, ModelArchitecture::Onnx);
    assert_ne!(got_gguf.sha256, got_onnx.sha256);
}

#[test]
fn dedicated_manifest_has_precedence_over_legacy_sibling() {
    let dir = fresh_dir("dedicated_beats_legacy");
    let path = dir.join("qwen.gguf");
    let dedicated = manifest(ModelArchitecture::Gguf, vec![ModelCapability::TextGeneration]);
    let legacy = manifest(ModelArchitecture::Onnx, vec![ModelCapability::Embedding]);
    std::fs::write(dir.join("manifest.json"), serde_json::to_string(&legacy).unwrap()).unwrap();
    std::fs::write(
        dir.join("qwen.gguf.manifest.json"),
        serde_json::to_string(&dedicated).unwrap(),
    ).unwrap();
    assert_eq!(
        read_sibling_manifest(&path).unwrap().unwrap().architecture,
        ModelArchitecture::Gguf
    );
}

#[test]
fn malformed_dedicated_manifest_must_not_fall_back_to_valid_legacy() {
    let dir = fresh_dir("dedicated_malformed");
    let path = dir.join("qwen.gguf");
    let legacy = manifest(ModelArchitecture::Gguf, vec![ModelCapability::TextGeneration]);
    std::fs::write(dir.join("manifest.json"), serde_json::to_string(&legacy).unwrap()).unwrap();
    std::fs::write(dir.join("qwen.gguf.manifest.json"), b"{NOT JSON").unwrap();
    let err = read_sibling_manifest(&path).unwrap_err().to_string();
    assert!(err.contains("invalid manifest JSON"), "got: {err}");
}

#[test]
fn legacy_manifest_still_reads_without_dedicated_entry() {
    let dir = fresh_dir("legacy_compatible");
    let path = dir.join("qwen.gguf");
    let m = manifest(ModelArchitecture::Gguf, vec![ModelCapability::TextGeneration]);
    std::fs::write(dir.join("manifest.json"), serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(read_sibling_manifest(&path).unwrap().unwrap().model_id, "m");
}

#[test]
fn manifest_size_is_bounded_before_parsing() {
    let dir = fresh_dir("manifest_size_limit");
    let path = dir.join("model.gguf");
    std::fs::write(dir.join("model.gguf.manifest.json"), vec![b' '; 1_048_577]).unwrap();
    let err = read_sibling_manifest(&path).unwrap_err().to_string();
    assert!(err.contains("exceeds 1 MiB"), "got: {err}");
}

#[cfg(unix)]
#[test]
fn symlinked_manifest_is_refused_even_when_legacy_is_valid() {
    use std::os::unix::fs::symlink;
    let dir = fresh_dir("manifest_link");
    let path = dir.join("model.gguf");
    let trusted = manifest(ModelArchitecture::Gguf, vec![ModelCapability::TextGeneration]);
    std::fs::write(dir.join("manifest.json"), serde_json::to_string(&trusted).unwrap()).unwrap();
    symlink(dir.join("manifest.json"), dir.join("model.gguf.manifest.json")).unwrap();
    let err = read_sibling_manifest(&path).unwrap_err().to_string();
    assert!(err.contains("regular non-symlink"), "got: {err}");
}

#[test]
fn directory_named_like_a_manifest_is_refused() {
    let dir = fresh_dir("manifest_directory");
    let path = dir.join("model.gguf");
    std::fs::create_dir_all(dir.join("model.gguf.manifest.json")).unwrap();
    let err = read_sibling_manifest(&path).unwrap_err().to_string();
    assert!(err.contains("regular non-symlink"), "got: {err}");
}
