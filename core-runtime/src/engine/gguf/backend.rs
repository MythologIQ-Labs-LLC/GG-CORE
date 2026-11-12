//! llama-cpp-2 backend for GGUF inference.
//!
//! Model loading, context creation, and token generation
//! via the llama-cpp-2 Rust bindings.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::engine::{FinishReason, GenerationResult, InferenceConfig, InferenceError};
use crate::security::stream_sanitizer::StreamSanitizer;

use super::prefill::{decode_prompt, decode_range, stop_requested, PrefillShape, StopCheck};

/// Holds the loaded llama-cpp-2 model and a handle to the process backend.
pub struct LlamaBackendInner {
    backend: &'static LlamaBackend,
    model: LlamaModel,
    pub(super) shape: super::working_memory::ContextShape,
    working_bytes: usize,
    n_threads: i32,
}

// SAFETY: LlamaModel and LlamaBackend are Send+Sync in llama-cpp-2.
unsafe impl Send for LlamaBackendInner {}
unsafe impl Sync for LlamaBackendInner {}

/// The process-wide llama.cpp backend.
///
/// llama-cpp-2 permits exactly one live `LlamaBackend` and frees it on drop.
/// Tying it to a model's lifetime made `unload` + `load` fail with
/// `BackendAlreadyInitialized` whenever any generation thread still held the
/// old model (for example right after a cancelled or expired stream, while the
/// native loop winds down to its next checkpoint). Initialize once and keep it
/// for the life of the process; model lifetimes then never depend on each
/// other's in-flight work.
fn process_backend() -> Result<&'static LlamaBackend, InferenceError> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    static INIT: Mutex<()> = Mutex::new(());
    if let Some(backend) = BACKEND.get() {
        return Ok(backend);
    }
    let _serialize = INIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(backend) = BACKEND.get() {
        return Ok(backend);
    }
    let backend = LlamaBackend::init()
        .map_err(|e| InferenceError::ModelError(format!("backend init: {e}")))?;
    Ok(BACKEND.get_or_init(|| backend))
}

impl LlamaBackendInner {
    /// Load a GGUF model from disk.
    pub fn load(path: &Path, config: &super::GgufConfig) -> Result<Self, InferenceError> {
        let backend = process_backend()?;
        let model_params = LlamaModelParams::default().with_n_gpu_layers(config.n_gpu_layers);
        let model = LlamaModel::load_from_file(backend, path, &model_params)
            .map_err(|e| InferenceError::ModelError(format!("load: {e}")))?;
        let (shape, working_bytes) = super::working_memory::shape_for(&model, config.n_ctx);
        let n_threads = resolve_threads(config.n_threads);
        Ok(Self {
            backend,
            model,
            shape,
            working_bytes,
            n_threads,
        })
    }

    /// Bytes one request's context can commit (admitted before llama.cpp).
    pub fn working_memory_estimate(&self) -> usize {
        self.working_bytes
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
    /// When `is_cancelled` is provided, it is checked before the context is
    /// created, between prompt-prefill decode chunks, and once per generated
    /// token. If set, generation stops early with `FinishReason::Cancelled`
    /// and the context (KV cache) is released on return.
    pub fn generate_cancellable(
        &self,
        prompt: &str,
        config: &InferenceConfig,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<GenerationResult, InferenceError> {
        let tokens = self.tokenize(prompt)?;
        let max_tok = config.max_tokens.unwrap_or(256);
        if stop_requested(is_cancelled) {
            return Ok(cancelled_result());
        }
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
        if stop_requested(is_cancelled) {
            return Ok(());
        }
        let mut ctx = self.create_context()?;
        ensure_prompt_room(&ctx, tokens.len())?;
        // llama.cpp rejects any single decode larger than ctx.n_batch(). A stop
        // between prefill chunks ends the stream; the caller reports the reason.
        if !self.prefill(&mut ctx, &tokens, is_cancelled)? {
            return Ok(());
        }
        let limit =
            (config.max_tokens.unwrap_or(256) as usize).min(ctx.n_ctx() as usize - tokens.len());
        let mut batch = LlamaBatch::new(1, 1);
        let mut sampler = build_sampler(config, self.model.n_vocab());
        sampler.accept_many(tokens.iter().copied());
        let start_pos = tokens.len() as i32;
        let rt = tokio::runtime::Handle::current();
        let mut acc: Vec<LlamaToken> = Vec::new();
        for offset in 0..limit {
            let pos = start_pos + offset as i32;
            if is_cancelled.as_ref().is_some_and(|f| f()) {
                break;
            }
            // Use -1 to sample from the last token that had logits computed
            let tok = sampler.sample(&ctx, -1);
            sampler.accept(tok);
            let eog = self.model.vocab().is_eog(tok);
            if !self.emit_token(&rt, sender, sanitizer.as_deref_mut(), tok, &mut acc)? {
                break;
            }
            if eog || offset + 1 == limit {
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
        ensure_prompt_room(&ctx, tokens.len())?;
        decode_range(&mut ctx, &tokens, 0, false)?;
        let mut batch = LlamaBatch::new(1, 1);
        let mut sampler = build_sampler(&config, self.model.n_vocab());
        sampler.accept_many(tokens.iter().copied());
        let mut out = Vec::with_capacity(count);
        let start_pos = tokens.len() as i32;
        let limit = count.min(ctx.n_ctx() as usize - tokens.len());
        for offset in 0..limit {
            let pos = start_pos + offset as i32;
            let tok = sampler.sample(&ctx, -1);
            sampler.accept(tok);
            if self.model.vocab().is_eog(tok) {
                break;
            }
            out.push(tok.0 as u32);
            if offset + 1 == limit {
                break;
            }
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

        if draft.is_empty() {
            return Ok(VerifyResult::accept_all(0));
        }
        if context.is_empty() {
            return Err(InferenceError::InputValidation(
                "speculative verification requires non-empty context".into(),
            ));
        }
        let mut ctx = self.create_context()?;
        let total = context.len().checked_add(draft.len()).ok_or_else(|| {
            InferenceError::InputValidation("speculative context length overflow".into())
        })?;
        if total > ctx.n_ctx() as usize {
            return Err(InferenceError::InputValidation(
                "speculative context exceeds model context window".into(),
            ));
        }
        // All draft logits must remain available after one decode. Decode the
        // preceding context in bounded chunks, then decode its last token
        // together with the draft. A huge draft fails without a native assert.
        if draft.len() >= ctx.n_batch().min(ctx.n_ubatch()) as usize {
            return Err(InferenceError::InputValidation(
                "speculative draft exceeds the supported decode batch".into(),
            ));
        }
        let context_tokens: Vec<LlamaToken> =
            context.iter().map(|&t| LlamaToken(t as i32)).collect();
        decode_range(&mut ctx, &context_tokens[..context.len() - 1], 0, false)?;
        let mut batch = LlamaBatch::new(draft.len() + 1, 1);
        batch
            .add(
                context_tokens[context.len() - 1],
                (context.len() - 1) as i32,
                &[0],
                true,
            )
            .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))?;
        for (i, &tok) in draft.iter().enumerate() {
            batch
                .add(
                    LlamaToken(tok as i32),
                    (context.len() + i) as i32,
                    &[0],
                    true,
                )
                .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))?;
        }
        decode(&mut ctx, &mut batch)?;
        let mut sampler = build_sampler(&InferenceConfig::default(), self.model.n_vocab());
        for (i, &draft_tok) in draft.iter().enumerate() {
            // A logit index is relative to this batch, not the absolute KV position.
            let predicted = sampler.sample(&ctx, i as i32);
            sampler.accept(predicted);
            if predicted.0 as u32 != draft_tok {
                return Ok(VerifyResult::diverge_at(i, predicted.0 as u32));
            }
        }
        Ok(VerifyResult::accept_all(draft.len()))
    }

    /// Get EOS token ID.
    pub fn eos_token(&self) -> Option<u32> {
        Some(self.model.vocab().eos().0 as u32)
    }

    /// Tokenize a prompt string (adds BOS; parses special tokens, matching
    /// the pre-0.1.158 `str_to_token(_, AddBos::Always)` behavior).
    pub fn tokenize(&self, text: &str) -> Result<Vec<LlamaToken>, InferenceError> {
        Ok(self.model.vocab().tokenize(text.as_bytes(), true, true))
    }

    /// Convert token IDs back to a string (lossy UTF-8, so multi-byte
    /// characters split across token boundaries decode correctly).
    pub fn detokenize(&self, tokens: &[LlamaToken]) -> Result<String, InferenceError> {
        let vocab = self.model.vocab();
        let mut bytes = Vec::new();
        for &t in tokens {
            bytes.extend_from_slice(&vocab.token_to_piece(t, false, None));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Plain prompt prefill on the deterministic schedule, stop or no stop
    /// (`prefill_schedule`): the split depends only on this model, the
    /// thread bucket and the prompt.
    pub(super) fn prefill(
        &self,
        ctx: &mut LlamaContext<'_>,
        tokens: &[LlamaToken],
        should_stop: StopCheck<'_>,
    ) -> Result<bool, InferenceError> {
        let shape = PrefillShape {
            threads: u32::try_from(self.n_threads).unwrap_or(1),
            model_bytes: self.model.size(),
        };
        decode_prompt(ctx, tokens, shape, should_stop)
    }

    pub(super) fn create_context(&self) -> Result<LlamaContext<'_>, InferenceError> {
        self.model
            .new_context(self.backend, self.shape.params(self.n_threads))
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
        ensure_prompt_room(ctx, tokens.len())?;
        if !self.prefill(ctx, tokens, is_cancelled)? {
            return Ok((Vec::new(), FinishReason::Cancelled));
        }
        let mut batch = LlamaBatch::new(1, 1);
        let mut sampler = build_sampler(config, self.model.n_vocab());
        sampler.accept_many(tokens.iter().copied());
        let mut out = Vec::new();
        let start_pos = tokens.len() as i32;
        // The final sampled token requires no extra decode once context is full.
        let limit = (max_tok as usize).min(ctx.n_ctx() as usize - tokens.len());
        for offset in 0..limit {
            let pos = start_pos + offset as i32;
            if is_cancelled.as_ref().is_some_and(|f| f()) {
                return Ok((out, FinishReason::Cancelled));
            }
            let tok = sampler.sample(ctx, -1);
            sampler.accept(tok);
            if self.model.vocab().is_eog(tok) {
                return Ok((out, FinishReason::Stop));
            }
            out.push(tok);
            if offset + 1 == limit {
                return Ok((out, FinishReason::MaxTokens));
            }
            batch.clear();
            add_one(&mut batch, tok, pos)?;
            decode(ctx, &mut batch)?;
        }
        Ok((out, FinishReason::MaxTokens))
    }
}

fn ensure_prompt_room(ctx: &LlamaContext<'_>, prompt_len: usize) -> Result<(), InferenceError> {
    if prompt_len == 0 {
        return Err(InferenceError::InputValidation(
            "prompt must contain at least one token".into(),
        ));
    }
    if prompt_len >= ctx.n_ctx() as usize {
        return Err(InferenceError::InputValidation(format!(
            "prompt contains {prompt_len} tokens, leaving no room in the {}-token context",
            ctx.n_ctx()
        )));
    }
    Ok(())
}

fn add_one(batch: &mut LlamaBatch, tok: LlamaToken, pos: i32) -> Result<(), InferenceError> {
    batch
        .add(tok, pos, &[0], true)
        .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))
}

pub(super) fn decode(
    ctx: &mut LlamaContext<'_>,
    batch: &mut LlamaBatch,
) -> Result<(), InferenceError> {
    ctx.decode(batch)
        .map_err(|e| InferenceError::ModelError(format!("decode: {e}")))
}

fn cancelled_result() -> GenerationResult {
    GenerationResult {
        text: String::new(),
        tokens_generated: 0,
        finish_reason: FinishReason::Cancelled,
    }
}

fn build_sampler(config: &InferenceConfig, n_vocab: i32) -> LlamaSampler {
    let mut s = Vec::new();
    if config.repetition_penalty > 1.0 {
        s.push(LlamaSampler::penalties(
            n_vocab,
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

/// `GgufConfig::n_threads` (0 = auto for one worker; the daemon dispatch path
/// resolves its divided value before load) through the single thread policy.
fn resolve_threads(n: u32) -> i32 {
    i32::try_from(super::threads::per_context_for_host(n, 1)).unwrap_or(4)
}

#[cfg(test)]
mod real_model_regression_tests {
    use super::super::GgufConfig;
    use super::*;

    #[test]
    fn real_model_long_prompt_beyond_batch() {
        // Skips on an LFS pointer unless GG_CORE_REQUIRE_REAL_MODEL=1.
        let Some(path) = super::super::test_fixture::real_model() else {
            return;
        };

        let gguf = GgufConfig {
            n_ctx: 2048,
            n_threads: 4,
            n_gpu_layers: 0,
        };
        let backend = LlamaBackendInner::load(path, &gguf).expect("real Qwen fixture must load");
        let prompt =
            "The brown fox walks beside a clear river under a quiet blue sky. ".repeat(100);
        let tokens = backend.tokenize(&prompt).expect("tokenize fixture prompt");
        let context = backend.create_context().expect("create fixture context");
        let batch_cap = context.n_batch().min(context.n_ubatch()) as usize;
        assert!(
            tokens.len() > 512,
            "fixture must exceed the old single-batch ceiling"
        );
        assert!(
            tokens.len() > batch_cap,
            "fixture must exercise multiple actual decode batches (cap: {batch_cap})"
        );
        assert!(
            tokens.len() < context.n_ctx() as usize,
            "fixture must remain inside the context window"
        );
        drop(context);
        let result = backend
            .generate(
                &prompt,
                &InferenceConfig {
                    max_tokens: Some(4),
                    ..InferenceConfig::default()
                },
            )
            .expect("long prompt must not terminate native inference");
        assert!(result.tokens_generated <= 4);

        // The other side of the batch/context distinction: a prompt that does
        // not fit in the configured context must return an ordinary Rust error.
        // Native llama.cpp must never receive an oversized decode request.
        let overlong =
            "The brown fox walks beside a clear river under a quiet blue sky. ".repeat(500);
        let overlong_tokens = backend
            .tokenize(&overlong)
            .expect("tokenize overflow fixture");
        assert!(
            overlong_tokens.len() >= 4096,
            "overflow fixture must exceed even the configured 4096-token context"
        );
        assert!(matches!(
            backend.generate(&overlong, &InferenceConfig::default()),
            Err(InferenceError::InputValidation(_))
        ));

        // Release the 2048-token model before loading the 4096-token one so the
        // test holds one fixture at a time (the llama backend is process-wide).
        drop(backend);

        // An actual Alden-style prompt also exceeds the old 2048-token
        // context, so bounded batching alone would not be enough. The
        // producer daemon must apply GG_CORE_N_CTX=4096 to model loading.
        let long_context = GgufConfig {
            n_ctx: 4096,
            ..gguf.clone()
        };
        let long_backend = LlamaBackendInner::load(path, &long_context)
            .expect("GGUF fixture with 4096-token context must load");
        let persona =
            "The brown fox walks beside a clear river under a quiet blue sky. ".repeat(220);
        let persona_tokens = long_backend
            .tokenize(&persona)
            .expect("tokenize Alden-class fixture");
        assert!(
            persona_tokens.len() > 2048,
            "Alden-class fixture must exceed the old default context"
        );
        assert!(
            persona_tokens.len() < long_context.n_ctx as usize,
            "fixture must fit in configured 4096-token context"
        );
        let persona_result = long_backend
            .generate(
                &persona,
                &InferenceConfig {
                    max_tokens: Some(4),
                    ..InferenceConfig::default()
                },
            )
            .expect("generation must work with the configured extended context");
        assert!(persona_result.tokens_generated <= 4);

        assert!(matches!(
            long_backend.generate(&overlong, &InferenceConfig::default()),
            Err(InferenceError::InputValidation(_))
        ));
        long_backend
            .generate("Answer in one word: hello.", &InferenceConfig::default())
            .expect("model remains usable after rejecting a long prompt");
    }
}

#[cfg(test)]
#[path = "backend_lifetime_tests.rs"]
mod lifetime_tests;

#[cfg(test)]
#[path = "backend_cancel_tests.rs"]
mod cancel_tests;

#[cfg(test)]
#[path = "backend_cancel_latency_tests.rs"]
mod cancel_latency_tests;
