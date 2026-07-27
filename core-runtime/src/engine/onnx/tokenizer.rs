//! Tokenizer for the ONNX embed/classify paths.
//!
//! Prefers a real WordPiece/subword tokenizer loaded **offline** from a sibling
//! `tokenizer.json` (HuggingFace `tokenizers`, `from_file` — no network path is
//! compiled in). When no `tokenizer.json` accompanies the model it degrades to a
//! deterministic hash tokenizer that is explicitly named and logged, so ONNX
//! inference never *silently* runs on a missing vocabulary.

use std::path::Path;

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
    /// On miss or parse error, warns and falls back (never fails the load).
    pub(super) fn for_model(model_path: &Path) -> Self {
        let tok_path = model_path.with_file_name("tokenizer.json");
        if !tok_path.exists() {
            tracing::warn!(
                model = %model_path.display(),
                "no sibling tokenizer.json; ONNX tokenization degraded to hash fallback"
            );
            return Self::HashFallback;
        }
        match tokenizers::Tokenizer::from_file(&tok_path) {
            Ok(tok) => Self::WordPiece(Box::new(tok)),
            Err(e) => {
                tracing::warn!(
                    path = %tok_path.display(),
                    error = %e,
                    "tokenizer.json failed to load; degrading to hash fallback"
                );
                Self::HashFallback
            }
        }
    }

    /// Encode text to model input ids. A `WordPiece` encode error degrades to the
    /// hash fallback for that call rather than panicking on the inference path.
    pub(super) fn encode(&self, text: &str) -> Vec<i64> {
        match self {
            Self::WordPiece(tok) => match tok.encode(text, true) {
                Ok(enc) => enc.get_ids().iter().map(|&id| i64::from(id)).collect(),
                Err(e) => {
                    tracing::warn!(error = %e, "WordPiece encode failed; using hash fallback");
                    hash_encode(text)
                }
            },
            Self::HashFallback => hash_encode(text),
        }
    }
}

/// Deterministic hash-based encoding wrapped with hard-coded BERT `[CLS]`/`[SEP]`.
/// NOT a real vocabulary — a degraded fallback only (formerly `simple_tokenize`).
fn hash_encode(text: &str) -> Vec<i64> {
    let mut ids = vec![101i64]; // [CLS]
    for word in text.split_whitespace() {
        let hash = word.bytes().fold(0u64, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u64::from(b))
        });
        ids.push((hash % 29_000 + 1_000) as i64);
    }
    ids.push(102); // [SEP]
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_fallback_is_deterministic_and_wraps_cls_sep() {
        let a = OnnxTokenizer::HashFallback.encode("hello world");
        let b = OnnxTokenizer::HashFallback.encode("hello world");
        assert_eq!(a, b, "hash fallback must be deterministic");
        assert_eq!(a.first(), Some(&101), "leading [CLS]");
        assert_eq!(a.last(), Some(&102), "trailing [SEP]");
        assert_eq!(a.len(), 4, "[CLS] + 2 words + [SEP]");
    }

    #[test]
    fn for_model_without_sibling_json_falls_back() {
        let dir = std::env::temp_dir();
        let missing = dir.join("gg_core_no_such_model_b28.onnx");
        assert!(matches!(
            OnnxTokenizer::for_model(&missing),
            OnnxTokenizer::HashFallback
        ));
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
        let resolved = OnnxTokenizer::for_model(&model_path);
        assert!(
            matches!(resolved, OnnxTokenizer::WordPiece(_)),
            "sibling tokenizer.json must load as WordPiece"
        );

        let ids = resolved.encode("hello world");
        assert_eq!(ids, vec![3, 4], "must map to real vocab ids, not hashes");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
