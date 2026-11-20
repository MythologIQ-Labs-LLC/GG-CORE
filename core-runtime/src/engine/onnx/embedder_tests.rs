//! Embedder tests (B-ONNX-1). The fixture-backed tests run against the tiny
//! committed ONNX model (`tests/fixtures/models/onnx/tiny-embedder/`, ~1 KB,
//! generated once by `scripts/gen_onnx_fixture.py` — nothing is downloaded at
//! test time). Golden values were computed by the same generator in float32.

use super::*;
use crate::engine::Model;

const DIM: usize = 8;
#[cfg(feature = "onnx")]
const MODEL_ID: &str = "tiny-embedder";

/// float32 reference: gather rows [3, 4] → masked mean → L2 normalize
/// (`scripts/gen_onnx_fixture.py` prints this as the "hello world" golden;
/// digits kept verbatim from that output, hence the precision allow).
#[cfg(feature = "onnx")]
#[allow(clippy::excessive_precision)]
const GOLDEN_HELLO_WORLD: [f32; DIM] = [
    0.31656653,
    -0.28370523,
    -0.46834815,
    0.033140063,
    0.48607799,
    0.22691055,
    -0.36468142,
    -0.42201424,
];

#[cfg(feature = "onnx")]
fn fixture_model_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/models/onnx")
}

#[cfg(feature = "onnx")]
fn load_fixture() -> OnnxEmbedder {
    OnnxEmbedder::load(&fixture_model_dir(), MODEL_ID, DIM).expect("committed fixture must load")
}

#[cfg(feature = "onnx")]
async fn embed_one(embedder: &OnnxEmbedder, text: &str) -> Vec<f32> {
    let input = InferenceInput::Text(text.to_string());
    match embedder.infer(&input, &InferenceConfig::default()).await {
        Ok(InferenceOutput::Embedding(r)) => r.vector,
        other => panic!("expected Embedding output, got {other:?}"),
    }
}

#[cfg(feature = "onnx")]
#[test]
fn committed_onnx_model_with_corrupt_tokenizer_refuses_loading() {
    let dir = tempfile::tempdir().expect("isolated model dir");
    let model_dir = dir.path().join(MODEL_ID);
    std::fs::create_dir(&model_dir).expect("make model directory");
    let fixture = fixture_model_dir().join(MODEL_ID).join("model.onnx");
    std::fs::copy(&fixture, model_dir.join("model.onnx")).expect("copy committed ONNX graph");
    std::fs::write(model_dir.join("tokenizer.json"), b"{ broken tokenizer json")
        .expect("write malformed sibling tokenizer");

    let err = OnnxEmbedder::load(dir.path(), MODEL_ID, DIM)
        .err()
        .expect("invalid tokenizer must prevent model load");
    assert!(
        matches!(err, InferenceError::ModelError(message) if message.contains("tokenizer.json")),
        "must refuse malformed tokenizer on real graph load"
    );
}

#[cfg(feature = "onnx")]
#[test]
fn public_onnx_classifier_loader_refuses_corrupt_tokenizer() {
    let dir = tempfile::tempdir().expect("isolated model dir");
    let path = dir.path().join("model.onnx");
    let fixture = fixture_model_dir().join(MODEL_ID).join("model.onnx");
    std::fs::copy(fixture, &path).expect("copy committed ONNX graph");
    std::fs::write(dir.path().join("tokenizer.json"), b"{ invalid")
        .expect("write invalid tokenizer");
    let error = super::super::load_onnx_classifier(
        &path,
        "classifier",
        vec!["negative".into(), "positive".into()],
        &super::super::OnnxConfig::default(),
    )
    .err()
    .expect("classifier loading must reject malformed tokenizer");
    assert!(
        matches!(error, InferenceError::ModelError(message) if message.contains("tokenizer.json")),
        "classifier cannot silently switch to synthetic hash vocabulary"
    );
}

#[cfg(feature = "onnx")]
#[tokio::test]
async fn golden_vector_hello_world() {
    let embedder = load_fixture();
    let vector = embed_one(&embedder, "hello world").await;
    assert_eq!(vector.len(), DIM);
    for (i, (got, want)) in vector.iter().zip(GOLDEN_HELLO_WORLD.iter()).enumerate() {
        assert!((got - want).abs() < 1e-5, "dim {i}: got {got}, want {want}");
    }
}

#[cfg(feature = "onnx")]
#[tokio::test]
async fn real_onnx_embedder_refuses_wrong_declared_dimensions() {
    let embedder = OnnxEmbedder::load(&fixture_model_dir(), MODEL_ID, DIM + 1)
        .expect("committed fixture must load even before inferred output shape is known");
    let result = embedder
        .infer(
            &InferenceInput::Text("hello world".into()),
            &InferenceConfig::for_embedding(),
        )
        .await;
    assert!(
        matches!(result, Err(InferenceError::ModelError(message)) if message.contains("dimension mismatch")),
        "a mismatched embedding width must not produce a success-shaped response"
    );
}

#[cfg(feature = "onnx")]
#[tokio::test]
async fn embedding_is_l2_normalized() {
    let embedder = load_fixture();
    let vector = embed_one(&embedder, "greatest good").await;
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "L2 norm = {norm}");
}

#[cfg(feature = "onnx")]
#[tokio::test]
async fn deterministic_across_two_loads() {
    let a = embed_one(&load_fixture(), "hello greatest world").await;
    let b = embed_one(&load_fixture(), "hello greatest world").await;
    assert_eq!(a, b, "two independent loads must embed identically");
}

#[cfg(feature = "onnx")]
#[tokio::test]
async fn batch_embeds_every_item_and_matches_per_item() {
    let embedder = load_fixture();
    let texts = ["hello world", "greatest good", "hello"];

    let batch = InferenceInput::TextBatch(texts.iter().map(|t| t.to_string()).collect());
    let results = match embedder.infer(&batch, &InferenceConfig::default()).await {
        Ok(InferenceOutput::EmbeddingBatch(rs)) => rs,
        other => panic!("expected EmbeddingBatch output, got {other:?}"),
    };
    assert_eq!(results.len(), texts.len(), "one result per batch item");

    for (text, result) in texts.iter().zip(&results) {
        assert_eq!(result.dimensions, DIM);
        let single = embed_one(&embedder, text).await;
        assert_eq!(result.vector, single, "batch item for {text:?} != per-item");
    }
}

#[cfg(feature = "onnx")]
#[tokio::test]
async fn memory_usage_tracks_load_and_unload() {
    let mut embedder = load_fixture();
    // 16x8 f32 table = 512 payload bytes at minimum.
    assert!(embedder.memory_usage() >= 512, "loaded footprint reported");
    embedder.unload().await.expect("unload");
    assert_eq!(embedder.memory_usage(), 0, "unload zeroes the footprint");
    let input = InferenceInput::Text("hello".into());
    assert!(
        embedder
            .infer(&input, &InferenceConfig::default())
            .await
            .is_err(),
        "inference after unload must fail loud"
    );
}

#[cfg(feature = "onnx")]
#[test]
fn load_missing_model_fails_cleanly() {
    let missing = fixture_model_dir().join("does-not-exist");
    let err = OnnxEmbedder::load(&missing, "nope", DIM);
    assert!(matches!(err, Err(InferenceError::ModelError(_))));
}

#[cfg(not(feature = "onnx"))]
#[test]
fn load_without_feature_fails_cleanly() {
    let err = OnnxEmbedder::load(std::path::Path::new("/nonexistent"), "any", DIM);
    assert!(matches!(err, Err(InferenceError::ModelError(_))));
}

#[tokio::test]
async fn not_loaded_embedder_errors_cleanly() {
    // `new()` is the not-loaded state under BOTH feature configurations.
    let embedder = OnnxEmbedder::new("missing".into(), DIM);
    let input = InferenceInput::Text("test".into());
    let result = embedder.infer(&input, &InferenceConfig::default()).await;
    assert!(matches!(result, Err(InferenceError::ModelError(_))));
}

#[test]
fn embedding_dim_is_reported() {
    assert_eq!(OnnxEmbedder::new("m".into(), 384).embedding_dim(), 384);
}

/// Real-model e2e against all-MiniLM-L6-v2 (fp32 export: 19 ops incl. Erf/
/// MatMul/Softmax — full `simple_eval` coverage check, unlike the Gather-only
/// tiny fixture). The ~90 MB model is gitignored; skips gracefully when absent
/// (same convention as the classifier's fixture test). Place the model at
/// `fixtures/models/onnx/all-MiniLM-L6-v2/{model.onnx,tokenizer.json}`.
#[cfg(feature = "onnx")]
#[tokio::test]
async fn load_and_embed_real_minilm() {
    let model_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/models/onnx");
    if !model_dir.join("all-MiniLM-L6-v2/model.onnx").exists() {
        eprintln!("skipping load_and_embed_real_minilm: gitignored model not present");
        return;
    }
    let embedder = OnnxEmbedder::load(&model_dir, "all-MiniLM-L6-v2", 384)
        .expect("real MiniLM model must load");
    assert!(
        embedder.memory_usage() > 80_000_000,
        "~90 MB of weights reported, got {}",
        embedder.memory_usage()
    );

    let cat = embed_one(&embedder, "the cat sits on the mat").await;
    assert_eq!(cat.len(), 384, "MiniLM embeds at 384 dims");
    let norm = cat.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "L2 norm = {norm}");
    assert_eq!(
        cat,
        embed_one(&embedder, "the cat sits on the mat").await,
        "deterministic"
    );

    // Golden cross-check: first dims of the reference embedding computed with
    // onnxruntime + the `tokenizers` library over the same files (padded-to-128
    // ids, mask-weighted mean pool, L2 normalize). Catches tokenizer/mask/
    // pooling drift, not just op coverage.
    const REFERENCE_CAT_PREFIX: [f32; 8] = [
        0.134891, -0.032063, -0.020335, 0.035901, -0.028333, 0.041502, 0.033159, 0.036606,
    ];
    for (i, (got, want)) in cat.iter().zip(REFERENCE_CAT_PREFIX.iter()).enumerate() {
        assert!(
            (got - want).abs() < 1e-3,
            "dim {i}: got {got}, onnxruntime reference {want}"
        );
    }

    // Semantic sanity: unit vectors, so cosine similarity == dot product.
    // onnxruntime reference: cat/kitten 0.6470, cat/quark -0.0415.
    let kitten = embed_one(&embedder, "a kitten rests on a rug").await;
    let quark = embed_one(&embedder, "quantum chromodynamics of gluons").await;
    let dot = |a: &[f32], b: &[f32]| -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() };
    let (near, far) = (dot(&cat, &kitten), dot(&cat, &quark));
    eprintln!("minilm cosine: cat/kitten = {near:.4}, cat/quark = {far:.4}");
    assert!((near - 0.6470).abs() < 0.02, "cat/kitten cosine = {near}");
    assert!((far - -0.0415).abs() < 0.02, "cat/quark cosine = {far}");
}
