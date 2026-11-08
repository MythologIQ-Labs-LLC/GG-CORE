//! Per-request working memory for a GGUF context, derived from model metadata.
//!
//! A request creates one llama.cpp context: an f16 KV cache sized by `n_ctx`
//! plus compute buffers sized by `n_ubatch`. [`ContextShape`] is the single
//! source of the context parameters *and* the estimate, so they cannot drift.
//! The estimate is admitted against the shared total before llama.cpp runs,
//! which keeps allocation failures (and llama.cpp's retention on its
//! compute-buffer failure path) from being reached at the Job Object cap.
//!
//! Fail-closed: the compute term always includes the non-flash-attention
//! score + softmax bound, and missing or implausible metadata selects a
//! conservative shape. Checked arithmetic; overflow yields `usize::MAX`
//! (never admissible).

/// Fixed micro-batch the context is created with (llama.cpp's default).
pub const N_UBATCH: u32 = 512;

const F16: u64 = 2;
const F32: u64 = 4;
const MARGIN_NUM: u64 = 9; // x1.125
const MARGIN_DEN: u64 = 8;
const OVERHEAD: u64 = 32 * 1024 * 1024; // output buffer, graph metadata, threads

/// Context parameters every request uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextShape {
    pub n_ctx: u32,
    pub n_ubatch: u32,
    /// Test-only override: force flash attention off (auto otherwise).
    pub flash_attn_disabled: bool,
}

impl ContextShape {
    pub fn new(n_ctx: u32) -> Self {
        Self {
            n_ctx,
            n_ubatch: N_UBATCH,
            flash_attn_disabled: false,
        }
    }
}

/// Model dimensions read from GGUF metadata.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelDims {
    pub n_layer: u64,
    pub n_embd: u64,
    pub n_head: u64,
    pub n_head_kv: u64,
    pub n_vocab: u64,
    pub n_ff: Option<u64>,
    pub head_k: Option<u64>,
    pub head_v: Option<u64>,
}

impl ModelDims {
    /// Upper-bound dims when metadata is missing or implausible.
    fn conservative(self) -> Self {
        let head = self.n_embd / self.n_head.max(1);
        Self {
            n_head: self.n_head.max(1),
            n_head_kv: self.n_head.max(1),
            n_ff: Some(self.n_embd.saturating_mul(8)),
            head_k: Some(self.head_k.unwrap_or(head).max(head)),
            head_v: Some(self.head_v.unwrap_or(head).max(head)),
            ..self
        }
    }

    fn complete(&self) -> bool {
        self.n_head > 0 && self.n_head_kv > 0 && self.n_ff.is_some() && self.n_embd > 0
    }
}

/// Bytes one request's context can commit (estimate >= real need).
pub fn working_memory_estimate(dims: &ModelDims, shape: &ContextShape) -> usize {
    let dims = if dims.complete() {
        *dims
    } else {
        dims.conservative()
    };
    raw_bytes(&dims, shape)
        .and_then(|b| b.checked_mul(MARGIN_NUM))
        .map(|b| b.div_ceil(MARGIN_DEN))
        .and_then(|b| b.checked_add(OVERHEAD))
        .and_then(|b| usize::try_from(b).ok())
        .unwrap_or(usize::MAX)
}

fn raw_bytes(d: &ModelDims, s: &ContextShape) -> Option<u64> {
    let (ctx, ub) = (u64::from(s.n_ctx), u64::from(s.n_ubatch));
    let head = d.n_embd / d.n_head.max(1);
    let per_token = d
        .head_k
        .unwrap_or(head)
        .checked_add(d.head_v.unwrap_or(head))?;
    let kv = d.n_layer.checked_mul(ctx)?.checked_mul(d.n_head_kv)?;
    let kv = kv.checked_mul(per_token)?.checked_mul(F16)?;
    let rows = d
        .n_vocab
        .checked_add(d.n_ff?)?
        .checked_add(d.n_embd.checked_mul(2)?)?;
    let activations = ub.checked_mul(rows)?.checked_mul(F32)?;
    // Non-flash attention: scores and their softmax may both be live.
    let scores = ub
        .checked_mul(ctx)?
        .checked_mul(d.n_head)?
        .checked_mul(F32 * 2)?;
    kv.checked_add(activations)?.checked_add(scores)
}

#[cfg(feature = "gguf")]
mod native {
    use std::num::NonZeroU32;

    use llama_cpp_2::context::params::{KvCacheType, LlamaContextParams};
    use llama_cpp_2::model::LlamaModel;

    use super::{ContextShape, ModelDims};

    /// `LLAMA_FLASH_ATTN_TYPE_DISABLED` (llama.h; bindgen maps the C enum
    /// `llama_flash_attn_type` to `c_int`). The sys crate is not re-exported.
    const FLASH_ATTN_DISABLED: std::os::raw::c_int = 0;

    impl ContextShape {
        /// llama.cpp context parameters for this shape.
        pub fn params(&self, n_threads: i32) -> LlamaContextParams {
            let p = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(self.n_ctx))
                .with_n_ubatch(self.n_ubatch)
                .with_type_k(KvCacheType::F16)
                .with_type_v(KvCacheType::F16)
                .with_n_threads(n_threads)
                .with_n_threads_batch(n_threads);
            if self.flash_attn_disabled {
                return p.with_flash_attention_policy(FLASH_ATTN_DISABLED);
            }
            p
        }
    }

    /// The context shape for `n_ctx` and this model's per-request estimate.
    pub fn shape_for(model: &LlamaModel, n_ctx: u32) -> (ContextShape, usize) {
        let shape = ContextShape::new(n_ctx);
        (
            shape,
            super::working_memory_estimate(&model_dims(model), &shape),
        )
    }

    /// Read the dimensions the estimate needs from a loaded model.
    pub fn model_dims(model: &LlamaModel) -> ModelDims {
        let arch = model.meta_val_str("general.architecture").ok();
        let meta = |key: &str| -> Option<u64> {
            let arch = arch.as_deref()?;
            let value = model.meta_val_str(&format!("{arch}.{key}")).ok()?;
            value.trim().parse().ok()
        };
        ModelDims {
            n_layer: u64::from(model.n_layer()),
            n_embd: u64::try_from(model.n_embd()).unwrap_or(0),
            n_head: u64::from(model.n_head()),
            n_head_kv: u64::from(model.n_head_kv()),
            n_vocab: u64::try_from(model.n_vocab()).unwrap_or(0),
            n_ff: meta("feed_forward_length"),
            head_k: meta("attention.key_length"),
            head_v: meta("attention.value_length"),
        }
    }
}

#[cfg(feature = "gguf")]
pub use native::{model_dims, shape_for};

#[cfg(test)]
#[path = "working_memory_tests.rs"]
mod tests;
