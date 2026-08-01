//! Tests for the model-free prompt-lookup draft (B-21f). No model required.

use super::PromptLookupDraft;
use crate::engine::adaptive_speculative::BlockDraftModel;

fn draft_of(context: &[u32], ngram: usize, max_draft: usize) -> Vec<u32> {
    let d = PromptLookupDraft::new(ngram, max_draft);
    futures::executor::block_on(d.draft(context, max_draft))
        .unwrap()
        .tokens
}

#[test]
fn proposes_continuation_after_ngram_match() {
    // needle = last 3 = [10,11,12]; earlier match at index 0 was followed by 99,98.
    let ctx = [10, 11, 12, 99, 98, 10, 11, 12];
    assert_eq!(draft_of(&ctx, 3, 8), vec![99, 98, 10, 11, 12]);
}

#[test]
fn empty_block_when_no_match() {
    // No trailing 3-gram repeats earlier.
    let ctx = [1, 2, 3, 4, 5];
    assert!(draft_of(&ctx, 3, 4).is_empty());
}

#[test]
fn respects_max_draft() {
    let ctx = [10, 11, 12, 99, 98, 97, 96, 10, 11, 12];
    let d = draft_of(&ctx, 3, 2);
    assert_eq!(d.len(), 2, "proposal capped at max_draft");
    assert_eq!(d, vec![99, 98]);
}

#[test]
fn empty_when_context_shorter_than_ngram() {
    let ctx = [1, 2];
    assert!(draft_of(&ctx, 3, 4).is_empty());
}
