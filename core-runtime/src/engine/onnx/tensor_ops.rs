//! Shared tensor helpers for the ONNX backend (feature `onnx` only).
//!
//! Single source of truth for the embedder's and classifier's input assembly
//! plus the embedding post-processing pipeline: deterministic output
//! selection, attention-mask-weighted mean pooling, and L2 normalization.

use std::collections::HashMap;

use candle_core::{DType, Device, Tensor};

use crate::engine::InferenceError;

/// Build `input_ids`, `attention_mask`, `token_type_ids` tensors (`[1, seq]`)
/// for a single tokenized sequence. The attention mask is the tokenizer's own
/// — when the tokenizer pads (MiniLM's `tokenizer.json` pads to 128), padded
/// positions carry mask 0 and must not be attended or pooled.
pub(super) fn build_transformer_inputs(
    encoded: &super::tokenizer::TokenizedInput,
    device: &Device,
) -> Result<HashMap<String, Tensor>, InferenceError> {
    let ids = Tensor::new(encoded.ids.as_slice(), device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| InferenceError::ModelError(format!("input: {e}")))?;

    let attn = Tensor::new(encoded.attention_mask.as_slice(), device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| InferenceError::ModelError(format!("attn: {e}")))?;

    let ttype =
        Tensor::zeros_like(&ids).map_err(|e| InferenceError::ModelError(format!("ttype: {e}")))?;

    let mut map = HashMap::new();
    map.insert("input_ids".to_string(), ids);
    map.insert("attention_mask".to_string(), attn);
    map.insert("token_type_ids".to_string(), ttype);
    Ok(map)
}

/// Select the hidden-state output tensor deterministically: prefer the named
/// `last_hidden_state`, else accept a single-output model. Fail loud on
/// ambiguity rather than picking a nondeterministic `HashMap` entry (mirrors
/// the classifier's `logits` rule).
pub(super) fn select_hidden_state(
    outputs: &HashMap<String, Tensor>,
) -> Result<&Tensor, InferenceError> {
    outputs
        .get("last_hidden_state")
        .or_else(|| (outputs.len() == 1).then(|| outputs.values().next().unwrap()))
        .ok_or_else(|| {
            InferenceError::ModelError(
                "ambiguous embedder outputs: expected a `last_hidden_state` output \
                 or exactly one output"
                    .into(),
            )
        })
}

/// Mean-pool hidden states over the attention mask.
///
/// `hidden` is `[1, seq, dim]`, `attention_mask` is `[1, seq]`; masked
/// positions contribute nothing and the sum is divided by the number of
/// attended tokens. Returns the pooled `[dim]` vector.
pub(super) fn masked_mean_pool(
    hidden: &Tensor,
    attention_mask: &Tensor,
) -> candle_core::Result<Tensor> {
    let mask = attention_mask.to_dtype(DType::F32)?.unsqueeze(2)?; // [1, seq, 1]
    let summed = hidden.to_dtype(DType::F32)?.broadcast_mul(&mask)?.sum(1)?; // [1, dim]
    let counts = mask.sum(1)?; // [1, 1]
    summed.broadcast_div(&counts)?.squeeze(0) // [dim]
}

/// L2-normalize an embedding into a finite unit vector.
/// A zero/non-finite vector is not a meaningful embedding and must not
/// produce a success-shaped response. Accumulate in f64: squaring finite
/// f32 inputs in f32 can overflow to infinity and normalize to all zeros.
pub(super) fn l2_normalize(vector: Vec<f32>) -> Result<Vec<f32>, InferenceError> {
    if vector.is_empty() || vector.iter().any(|v| !v.is_finite()) {
        return Err(InferenceError::ModelError(
            "embedding contains no values or non-finite values".into(),
        ));
    }
    let squared_norm: f64 = vector
        .iter()
        .map(|value| {
            let x = f64::from(*value);
            x * x
        })
        .sum();
    if squared_norm == 0.0 || !squared_norm.is_finite() {
        return Err(InferenceError::ModelError(
            "embedding has zero or invalid L2 norm".into(),
        ));
    }
    let norm = squared_norm.sqrt();
    Ok(vector
        .into_iter()
        .map(|value| (f64::from(value) / norm) as f32)
        .collect())
}

/// Approximate in-memory footprint of a loaded `ModelProto`: the sum of all
/// graph initializer payloads (weights dominate; graph structure is noise).
pub(super) fn model_memory_estimate(model: &candle_onnx::onnx::ModelProto) -> usize {
    model
        .graph
        .as_ref()
        .map(|g| g.initializer.iter().map(tensor_payload_bytes).sum())
        .unwrap_or(0)
}

/// Payload size of one initializer tensor across the ONNX storage variants.
fn tensor_payload_bytes(t: &candle_onnx::onnx::TensorProto) -> usize {
    t.raw_data.len()
        + t.float_data.len() * 4
        + t.int32_data.len() * 4
        + t.int64_data.len() * 8
        + t.double_data.len() * 8
        + t.uint64_data.len() * 8
        + t.string_data.iter().map(|s| s.len()).sum::<usize>()
}

#[cfg(test)]
mod normalization_tests {
    use super::*;

    #[test]
    fn large_finite_embeddings_normalize_without_f32_square_overflow() {
        let vector = l2_normalize(vec![f32::MAX, f32::MAX])
            .expect("large finite input must not become a zero embedding");
        assert_eq!(vector.len(), 2);
        for component in vector {
            assert!(component.is_finite());
            assert!((component - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        }
    }

    #[test]
    fn zero_empty_and_nonfinite_embeddings_fail_closed() {
        for vector in [
            vec![],
            vec![0.0_f32, 0.0],
            vec![f32::NAN, 1.0],
            vec![f32::INFINITY, 1.0],
            vec![f32::NEG_INFINITY, 1.0],
        ] {
            assert!(
                matches!(l2_normalize(vector), Err(InferenceError::ModelError(_))),
                "invalid embedding should never become a success-shaped vector"
            );
        }
    }

    #[test]
    fn tiny_finite_embeddings_normalize_without_underflow() {
        let result = l2_normalize(vec![f32::MIN_POSITIVE, 0.0])
            .expect("tiny nonzero embedding");
        assert_eq!(result, vec![1.0, 0.0]);
    }
}
