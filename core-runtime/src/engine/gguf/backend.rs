//! llama-cpp-2 backend for GGUF inference.
//!
//! Model loading, context creation, and token generation
//! via the llama-cpp-2 Rust bindings.

use std::num::NonZeroU32;
use std::path::Path;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::engine::{FinishReason, GenerationResult, InferenceConfig, InferenceError};
use crate::security::stream_sanitizer::StreamSanitizer;

/// Holds the loaded llama-cpp-2 model and backend.
pub struct LlamaBackendInner {
    backend: LlamaBackend,
    model: LlamaModel,
    n_ctx: u32,
    n_threads: i32,
}

// SAFETY: LlamaModel and LlamaBackend are Send+Sync in llama-cpp-2.
unsafe impl Send for LlamaBackendInner {}
unsafe impl Sync for LlamaBackendInner {}

impl LlamaBackendInner {
    /// Load a GGUF model from disk.
    pub fn load(path: &Path, config: &super::GgufConfig) -> Result<Self, InferenceError> {
        let backend = LlamaBackend::init()
            .map_err(|e| InferenceError::ModelError(format!("backend init: {e}")))?;
        let model_params = LlamaModelParams::default().with_n_gpu_layers(config.n_gpu_layers);
        let model = LlamaModel::load_from_file(&backend, path, &model_params)
            .map_err(|e| InferenceError::ModelError(format!("load: {e}")))?;
        let n_threads = resolve_threads(config.n_threads);
        Ok(Self {
            backend,
            model,
            n_ctx: config.n_ctx,
            n_threads,
        })
    }

    pub fn model_size(&self) -> usize {
        self.model.size() as usize
    }

    /// Generate text from a prompt using llama-cpp-2.
    pub fn generate(
        &self,
        prompt: &str,
        config: &InferenceConfig,
    ) -> Result<GenerationResult, InferenceError> {
        self.generate_cancellable(prompt, config, None)
    }

    /// Generate text with optional cooperative cancellation.
    ///
    /// When `is_cancelled` is provided, it is checked once per token.
    /// If set, generation stops early with `FinishReason::Cancelled`.
    pub fn generate_cancellable(
        &self,
        prompt: &str,
        config: &InferenceConfig,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<GenerationResult, InferenceError> {
        let tokens = self.tokenize(prompt)?;
        let max_tok = config.max_tokens.unwrap_or(256);
        let mut ctx = self.create_context()?;
        let (out_tokens, reason) =
            self.sample_loop(&mut ctx, &tokens, max_tok, config, is_cancelled)?;
        let text = self.detokenize(&out_tokens)?;
        let count = u32::try_from(out_tokens.len()).unwrap_or(u32::MAX);
        Ok(GenerationResult {
            text,
            tokens_generated: count,
            finish_reason: reason,
        })
    }

    /// Stream tokens one at a time through a channel.
    ///
    /// When `is_cancelled` is provided, it is checked once per token.
    /// If set, streaming stops early.
    pub fn generate_stream(
        &self,
        prompt: &str,
        config: &InferenceConfig,
        sender: &crate::engine::TokenStreamSender,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
        mut sanitizer: Option<&mut StreamSanitizer>,
    ) -> Result<(), InferenceError> {
        let tokens = self.tokenize(prompt)?;
        let max_tok = config.max_tokens.unwrap_or(256);
        let mut ctx = self.create_context()?;
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        add_seq(&mut batch, &tokens)?;
        decode(&mut ctx, &mut batch)?;
        let mut sampler = build_sampler(config);
        sampler.accept_many(tokens.iter().copied());
        let start_pos = tokens.len() as i32;
        let rt = tokio::runtime::Handle::current();
        let mut acc: Vec<LlamaToken> = Vec::new();
        for (offset, i) in (0..max_tok).enumerate() {
            let pos = start_pos + offset as i32;
            if is_cancelled.as_ref().is_some_and(|f| f()) {
                break;
            }
            // Use -1 to sample from the last token that had logits computed
            let tok = sampler.sample(&ctx, -1);
            sampler.accept(tok);
            let eog = self.model.is_eog_token(tok);
            if !self.emit_token(&rt, sender, sanitizer.as_deref_mut(), tok, &mut acc)? {
                break;
            }
            if eog || i + 1 == max_tok {
                break;
            }
            batch.clear();
            add_one(&mut batch, tok, pos)?;
            decode(&mut ctx, &mut batch)?;
        }
        self.flush_sanitizer(&rt, sender, sanitizer, &acc)
    }

    /// Emit one generated token. With a `sanitizer` present, the token is
    /// accumulated, detokenized, and egress-sanitized, and sanitized *text* is
    /// emitted so raw token ids never leave the runtime (B-24b); otherwise the raw
    /// token id is emitted. Returns `false` if the receiver is gone.
    fn emit_token(
        &self,
        rt: &tokio::runtime::Handle,
        sender: &crate::engine::TokenStreamSender,
        sanitizer: Option<&mut StreamSanitizer>,
        tok: LlamaToken,
        acc: &mut Vec<LlamaToken>,
    ) -> Result<bool, InferenceError> {
        match sanitizer {
            Some(san) => {
                acc.push(tok);
                let text = self.detokenize(acc)?;
                match san.push(&text) {
                    Some(chunk) => Ok(rt.block_on(sender.text(chunk)).is_ok()),
                    None => Ok(true),
                }
            }
            None => Ok(rt.block_on(sender.token(tok.0 as u32)).is_ok()),
        }
    }

    /// Terminal: sanitize and emit any withheld tail (B-24b flush).
    fn flush_sanitizer(
        &self,
        rt: &tokio::runtime::Handle,
        sender: &crate::engine::TokenStreamSender,
        sanitizer: Option<&mut StreamSanitizer>,
        acc: &[LlamaToken],
    ) -> Result<(), InferenceError> {
        if let Some(san) = sanitizer {
            let text = self.detokenize(acc)?;
            if let Some(tail) = san.flush(&text) {
                let _ = rt.block_on(sender.text(tail));
            }
        }
        Ok(())
    }

    /// Generate N tokens from token IDs (for speculative decoding).
    pub fn generate_from_tokens(
        &self,
        context: &[u32],
        count: usize,
    ) -> Result<Vec<u32>, InferenceError> {
        let tokens: Vec<LlamaToken> = context.iter().map(|&t| LlamaToken(t as i32)).collect();
        let config = InferenceConfig::default();
        let mut ctx = self.create_context()?;
        let mut batch = LlamaBatch::new(tokens.len().max(1), 1);
        add_seq(&mut batch, &tokens)?;
        decode(&mut ctx, &mut batch)?;
        let mut sampler = build_sampler(&config);
        sampler.accept_many(tokens.iter().copied());
        let mut out = Vec::with_capacity(count);
        let start_pos = tokens.len() as i32;
        for (offset, _) in (0..count).enumerate() {
            let pos = start_pos + offset as i32;
            let tok = sampler.sample(&ctx, -1);
            sampler.accept(tok);
            if self.model.is_eog_token(tok) {
                break;
            }
            out.push(tok.0 as u32);
            batch.clear();
            add_one(&mut batch, tok, pos)?;
            decode(&mut ctx, &mut batch)?;
        }
        Ok(out)
    }

    /// Verify draft tokens against model (for speculative decoding).
    #[cfg(feature = "advanced")]
    pub fn verify_tokens(
        &self,
        context: &[u32],
        draft: &[u32],
    ) -> Result<crate::engine::speculative_types::VerifyResult, InferenceError> {
        use crate::engine::speculative_types::VerifyResult;
        let all_tokens: Vec<LlamaToken> = context
            .iter()
            .chain(draft.iter())
            .map(|&t| LlamaToken(t as i32))
            .collect();
        let config = InferenceConfig::default();
        let mut ctx = self.create_context()?;
        // Add all tokens with logits enabled for verification positions
        let mut batch = LlamaBatch::new(all_tokens.len(), 1);
        let ctx_len = context.len();
        for (i, &tok) in all_tokens.iter().enumerate() {
            // Enable logits for context's last token and all draft positions
            let needs_logits = i >= ctx_len.saturating_sub(1);
            batch
                .add(tok, i as i32, &[0], needs_logits)
                .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))?;
        }
        decode(&mut ctx, &mut batch)?;
        let mut sampler = build_sampler(&config);
        // Verify each draft token
        for (i, &draft_tok) in draft.iter().enumerate() {
            let logit_idx = (ctx_len - 1 + i) as i32;
            let predicted = sampler.sample(&ctx, logit_idx);
            sampler.accept(predicted);
            if predicted.0 as u32 != draft_tok {
                return Ok(VerifyResult::diverge_at(i, predicted.0 as u32));
            }
        }
        Ok(VerifyResult::accept_all(draft.len()))
    }

    /// Get EOS token ID.
    pub fn eos_token(&self) -> Option<u32> {
        Some(self.model.token_eos().0 as u32)
    }

    /// Tokenize a prompt string.
    pub fn tokenize(&self, text: &str) -> Result<Vec<LlamaToken>, InferenceError> {
        self.model
            .str_to_token(text, AddBos::Always)
            .map_err(|e| InferenceError::InputValidation(format!("tokenize: {e}")))
    }

    /// Convert token IDs back to a string.
    pub fn detokenize(&self, tokens: &[LlamaToken]) -> Result<String, InferenceError> {
        let mut dec = encoding_rs::UTF_8.new_decoder();
        let mut out = String::new();
        for &t in tokens {
            let piece = self
                .model
                .token_to_piece(t, &mut dec, false, None)
                .map_err(|e| InferenceError::ModelError(format!("detok: {e}")))?;
            out.push_str(&piece);
        }
        Ok(out)
    }

    pub(super) fn create_context(&self) -> Result<LlamaContext<'_>, InferenceError> {
        // Use same thread count for both - simpler and avoids cache contention
        // llama.cpp internally optimizes based on workload
        let p = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx))
            .with_n_threads(self.n_threads)
            .with_n_threads_batch(self.n_threads);
        self.model
            .new_context(&self.backend, p)
            .map_err(|e| InferenceError::ModelError(format!("ctx: {e}")))
    }

    fn sample_loop(
        &self,
        ctx: &mut LlamaContext<'_>,
        tokens: &[LlamaToken],
        max_tok: u32,
        config: &InferenceConfig,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<(Vec<LlamaToken>, FinishReason), InferenceError> {
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        add_seq(&mut batch, tokens)?;
        decode(ctx, &mut batch)?;
        let mut sampler = build_sampler(config);
        sampler.accept_many(tokens.iter().copied());
        let mut out = Vec::new();
        let start_pos = tokens.len() as i32;
        for (offset, _) in (0..max_tok).enumerate() {
            let pos = start_pos + offset as i32;
            if is_cancelled.as_ref().is_some_and(|f| f()) {
                return Ok((out, FinishReason::Cancelled));
            }
            // Use -1 to sample from the last token that had logits computed
            let tok = sampler.sample(ctx, -1);
            sampler.accept(tok);
            if self.model.is_eog_token(tok) {
                return Ok((out, FinishReason::Stop));
            }
            out.push(tok);
            batch.clear();
            add_one(&mut batch, tok, pos)?;
            decode(ctx, &mut batch)?;
        }
        Ok((out, FinishReason::MaxTokens))
    }
}

fn add_seq(batch: &mut LlamaBatch, tokens: &[LlamaToken]) -> Result<(), InferenceError> {
    // Add all tokens except the last with logits=false
    // Add the last token with logits=true so we can sample from it
    let n = tokens.len();
    if n == 0 {
        return Ok(());
    }
    for (i, &tok) in tokens.iter().enumerate() {
        let logits = i == n - 1; // Only compute logits for last token
        batch
            .add(tok, i as i32, &[0], logits)
            .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))?;
    }
    Ok(())
}

fn add_one(batch: &mut LlamaBatch, tok: LlamaToken, pos: i32) -> Result<(), InferenceError> {
    batch
        .add(tok, pos, &[0], true)
        .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))
}

fn decode(ctx: &mut LlamaContext<'_>, batch: &mut LlamaBatch) -> Result<(), InferenceError> {
    ctx.decode(batch)
        .map_err(|e| InferenceError::ModelError(format!("decode: {e}")))
}

/// Decode `tokens` into `ctx` at positions `start_pos..start_pos+len` (single sequence 0).
/// `logits_all` enables logits on every position (verification); otherwise only the last
/// (generation / prefix commit). Used by the persistent speculative session (B-21f).
#[cfg(feature = "advanced")]
pub(super) fn decode_range(
    ctx: &mut LlamaContext<'_>,
    tokens: &[LlamaToken],
    start_pos: i32,
    logits_all: bool,
) -> Result<(), InferenceError> {
    if tokens.is_empty() {
        return Ok(());
    }
    let mut batch = LlamaBatch::new(tokens.len(), 1);
    let last = tokens.len() - 1;
    for (i, &tok) in tokens.iter().enumerate() {
        let logits = logits_all || i == last;
        batch
            .add(tok, start_pos + i as i32, &[0], logits)
            .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))?;
    }
    decode(ctx, &mut batch)
}

fn build_sampler(config: &InferenceConfig) -> LlamaSampler {
    let mut s = Vec::new();
    if config.repetition_penalty > 1.0 {
        s.push(LlamaSampler::penalties(
            64,
            config.repetition_penalty,
            0.0,
            0.0,
        ));
    }
    if config.top_k > 0 {
        s.push(LlamaSampler::top_k(config.top_k as i32));
    }
    s.push(LlamaSampler::top_p(config.top_p, 1));
    s.push(LlamaSampler::temp(config.temperature));
    s.push(LlamaSampler::dist(42));
    LlamaSampler::chain_simple(s)
}

fn resolve_threads(n: u32) -> i32 {
    if n == 0 {
        // LLM inference is memory-bound, hyperthreads help hide latency
        // Use all logical cores for small models, cap for large models
        let logical = num_cpus::get();
        // Cap at 16 to avoid diminishing returns on high-core systems
        let optimal = logical.clamp(1, 16);
        i32::try_from(optimal).unwrap_or(4)
    } else {
        i32::try_from(n).unwrap_or(4)
    }
}
