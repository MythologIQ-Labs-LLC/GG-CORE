//! Model-free prompt-lookup draft (B-21f).
//!
//! A `BlockDraftModel` that costs no model forward pass: it proposes the tokens
//! that followed the most recent earlier occurrence of the context's trailing
//! n-gram. On repetitive / extractive generation this yields a genuine speedup
//! (the expensive target verifies a free draft), demonstrable on a single model.
//! Any wrong proposal is simply rejected by the verifier, so the draft may be
//! liberal — correctness rests entirely on the target.

use async_trait::async_trait;

use super::{BlockDraftModel, DraftBlock};
use crate::engine::InferenceError;

/// Proposes draft tokens by copying the continuation of the last matching n-gram.
pub struct PromptLookupDraft {
    /// Length of the trailing n-gram used as the search needle.
    pub ngram: usize,
    /// Maximum number of tokens to propose.
    pub max_draft: usize,
}

impl PromptLookupDraft {
    pub fn new(ngram: usize, max_draft: usize) -> Self {
        Self { ngram, max_draft }
    }
}

#[async_trait]
impl BlockDraftModel for PromptLookupDraft {
    async fn draft(&self, context: &[u32], max: usize) -> Result<DraftBlock, InferenceError> {
        let k = self.max_draft.min(max);
        Ok(DraftBlock::from_tokens(lookup(context, self.ngram, k)))
    }
}

/// Return up to `k` tokens that followed the most recent earlier occurrence of the
/// context's trailing `ngram`, or empty if there is no match.
fn lookup(context: &[u32], ngram: usize, k: usize) -> Vec<u32> {
    if ngram == 0 || k == 0 || context.len() <= ngram {
        return Vec::new();
    }
    let needle = &context[context.len() - ngram..];
    // Candidate match starts run over 0..(len-ngram), excluding the needle itself.
    for start in (0..context.len() - ngram).rev() {
        if &context[start..start + ngram] == needle {
            let follow = start + ngram;
            let end = (follow + k).min(context.len());
            return context[follow..end].to_vec();
        }
    }
    Vec::new()
}

#[cfg(test)]
#[path = "prompt_lookup_tests.rs"]
mod prompt_lookup_tests;
