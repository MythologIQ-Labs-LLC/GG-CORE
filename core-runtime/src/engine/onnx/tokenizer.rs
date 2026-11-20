//! Tokenizer for the ONNX embed/classify paths.
//!
//! Prefers a real WordPiece/subword tokenizer loaded **offline** from a sibling
//! `tokenizer.json` (HuggingFace `tokenizers`, `from_file` — no network path is
//! compiled in). When no `tokenizer.json` accompanies the model it degrades to a
//! deterministic hash tokenizer that is explicitly named and logged, so ONNX
//! inference never *silently* runs on a missing vocabulary. A present but
//! corrupt or inaccessible tokenizer is a load error, never permission to
//! synthesize different token IDs.

use std::path::Path;

use crate::engine::InferenceError;

/// Token ids plus their attention mask. The mask is the tokenizer's own
/// (real tokenizers may pad to a fixed length — e.g. the MiniLM
/// `tokenizer.json` pads to 128 — and padded positions must not be attended
/// or pooled), never fabricated downstream.
pub(super) struct TokenizedInput {
    pub(super) ids: Vec<i64>,
    pub(super) attention_mask: Vec<i64>,
}

/// Tokenizer used by the ONNX models.
pub(super) enum OnnxTokenizer {
    /// Real subword tokenizer from a local `tokenizer.json` (offline).
    WordPiece(Box<tokenizers::Tokenizer>),
    /// Deterministic hash fallback — degraded, NOT a real vocabulary. Used only
    /// when no `tokenizer.json` is found next to the model.
    HashFallback,
}

impl OnnxTokenizer {
    /// Resolve a tokenizer for a model file by loading a sibling `tokenizer.json`
    /// (same directory). Offline: `Tokenizer::from_file` reads local disk only.
    /// A truly absent tokenizer retains the documented degraded hash fallback.
    /// A present malformed tokenizer, symlink, directory, or unreadable file
    /// is an error: otherwise model output can silently use the wrong vocabulary.
    pub(super) fn for_model(model_path: &Path) -> Result<Self, InferenceError> {
        let tok_path = model_path.with_file_name("tokenizer.json");
        match std::fs::symlink_metadata(&tok_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    model = %model_path.display(),
                    "no sibling tokenizer.json; ONNX tokenization degraded to hash fallback"
                );
                return Ok(Self::HashFallback);
            }
            Err(e) => {
                return Err(InferenceError::ModelError(format!(
                    "cannot inspect ONNX tokenizer.json: {e}"
                )));
            }
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                return Err(InferenceError::ModelError(
                    "ONNX tokenizer.json must be a regular non-symlink file".into(),
                ));
            }
            Ok(_) => {}
        }
        tokenizers::Tokenizer::from_file(&tok_path)
            .map(|tok| Self::WordPiece(Box::new(tok)))
            .map_err(|e| InferenceError::ModelError(format!("ONNX tokenizer.json is invalid: {e}")))
    }

    /// A configured real tokenizer must never downgrade to synthetic hash
    /// token IDs for an individual failed encoding.
    pub(super) fn encode(&self, text: &str) -> Result<TokenizedInput, InferenceError> {
        match self {
            Self::WordPiece(tok) => tok
                .encode(text, true)
                .map(|enc| TokenizedInput {
                    ids: enc.get_ids().iter().map(|&id| i64::from(id)).collect(),
                    attention_mask: enc
                        .get_attention_mask()
                        .iter()
                        .map(|&m| i64::from(m))
                        .collect(),
                })
                .map_err(|e| InferenceError::ModelError(format!("ONNX tokenization failed: {e}"))),
            Self::HashFallback => Ok(hash_encode(text)),
        }
    }
}

/// Deterministic hash-based encoding wrapped with hard-coded BERT `[CLS]`/`[SEP]`.
/// NOT a real vocabulary — a degraded fallback only (formerly `simple_tokenize`).
/// Never pads, so its attention mask is all ones.
fn hash_encode(text: &str) -> TokenizedInput {
    let mut ids = vec![101i64]; // [CLS]
    for word in text.split_whitespace() {
        let hash = word.bytes().fold(0u64, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u64::from(b))
        });
        ids.push((hash % 29_000 + 1_000) as i64);
    }
    ids.push(102); // [SEP]
    let attention_mask = vec![1i64; ids.len()];
    TokenizedInput {
        ids,
        attention_mask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_fallback_is_deterministic_and_wraps_cls_sep() {
        let a = OnnxTokenizer::HashFallback.encode("hello world").unwrap();
        let b = OnnxTokenizer::HashFallback.encode("hello world").unwrap();
        assert_eq!(a.ids, b.ids, "hash fallback must be deterministic");
        assert_eq!(a.ids.first(), Some(&101), "leading [CLS]");
        assert_eq!(a.ids.last(), Some(&102), "trailing [SEP]");
        assert_eq!(a.ids.len(), 4, "[CLS] + 2 words + [SEP]");
        assert_eq!(a.attention_mask, vec![1; 4], "unpadded mask is all ones");
    }

    #[test]
    fn for_model_without_sibling_json_falls_back() {
        let dir = std::env::temp_dir();
        let missing = dir.join("gg_core_no_such_model_b28.onnx");
        assert!(matches!(
            OnnxTokenizer::for_model(&missing),
            Ok(OnnxTokenizer::HashFallback)
        ));
    }

    #[test]
    fn present_invalid_tokenizer_never_downgrades_to_hash_ids() {
        let dir = tempfile::tempdir().expect("temp dir");
        let model_path = dir.path().join("model.onnx");
        std::fs::write(dir.path().join("tokenizer.json"), b"{malformed json")
            .expect("write malformed tokenizer");
        let result = OnnxTokenizer::for_model(&model_path);
        assert!(
            matches!(result, Err(InferenceError::ModelError(message)) if message.contains("invalid")),
            "a present malformed tokenizer must fail loading"
        );
    }

    #[test]
    fn directory_instead_of_tokenizer_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("tokenizer.json")).expect("make directory");
        let result = OnnxTokenizer::for_model(&dir.path().join("model.onnx"));
        assert!(
            matches!(result, Err(InferenceError::ModelError(message)) if message.contains("regular")),
            "a directory must not count as an absent tokenizer"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_tokenizer_symlink_is_not_treated_as_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::os::unix::fs::symlink(
            dir.path().join("missing-tokenizer.json"),
            dir.path().join("tokenizer.json"),
        )
        .expect("create dangling link");
        let result = OnnxTokenizer::for_model(&dir.path().join("model.onnx"));
        assert!(
            matches!(result, Err(InferenceError::ModelError(message)) if message.contains("non-symlink")),
            "a dangling symlink must not trigger degraded hash tokenization"
        );
    }

    #[test]
    fn wordpiece_loads_offline_and_encodes_real_vocab_ids() {
        // A minimal valid WordPiece tokenizer.json with a whitespace
        // pre-tokenizer. Written to disk and reloaded via `for_model`, exercising
        // the exact offline `from_file` path (no network, no Hub).
        let json_body = r###"{
  "version": "1.0",
  "truncation": null,
  "padding": null,
  "added_tokens": [],
  "normalizer": null,
  "pre_tokenizer": { "type": "Whitespace" },
  "post_processor": null,
  "decoder": null,
  "model": {
    "type": "WordPiece",
    "unk_token": "[UNK]",
    "continuing_subword_prefix": "##",
    "max_input_chars_per_word": 100,
    "vocab": { "[UNK]": 0, "[CLS]": 1, "[SEP]": 2, "hello": 3, "world": 4 }
  }
}"###;

        let dir = std::env::temp_dir().join("gg_core_b28_wp_fixture");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tokenizer.json"), json_body).unwrap();

        let model_path = dir.join("model.onnx");
        let resolved = OnnxTokenizer::for_model(&model_path).expect("valid tokenizer must load");
        assert!(
            matches!(resolved, OnnxTokenizer::WordPiece(_)),
            "sibling tokenizer.json must load as WordPiece"
        );

        let encoded = resolved
            .encode("hello world")
            .expect("real tokenizer must encode");
        assert_eq!(
            encoded.ids,
            vec![3, 4],
            "must map to real vocab ids, not hashes"
        );
        assert_eq!(
            encoded.attention_mask,
            vec![1, 1],
            "mask covers exactly the real tokens"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
