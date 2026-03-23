//! TDD-Light tests for speculative decoding.

use async_trait::async_trait;
use gg_core::engine::{
    DraftModel, InferenceError, SpeculativeConfig, SpeculativeDecoder, TargetModel, VerifyResult,
};

// Mock draft model for testing
struct MockDraft {
    tokens: Vec<u32>,
}

#[async_trait]
impl DraftModel for MockDraft {
    async fn generate_draft(
        &self,
        _context: &[u32],
        count: usize,
    ) -> Result<Vec<u32>, InferenceError> {
        Ok(self.tokens.iter().take(count).copied().collect())
    }
}

// Mock target model for testing
struct MockTarget {
    accept_count: usize,
    correction: Option<u32>,
    fallback_token: u32,
    eos: Option<u32>,
}

#[async_trait]
impl TargetModel for MockTarget {
    async fn verify_tokens(
        &self,
        _context: &[u32],
        _draft: &[u32],
    ) -> Result<VerifyResult, InferenceError> {
        Ok(VerifyResult {
            accepted_count: self.accept_count,
            correction_token: self.correction,
        })
    }

    async fn generate_one(&self, _context: &[u32]) -> Result<u32, InferenceError> {
        Ok(self.fallback_token)
    }

    fn eos_token(&self) -> Option<u32> {
        self.eos
    }
}

#[tokio::test]
async fn speculative_accepts_matching_draft() {
    let draft = MockDraft { tokens: vec![1, 2, 3, 4] };
    let target = MockTarget {
        accept_count: 4,
        correction: None,
        fallback_token: 99,
        eos: Some(0),
    };
    let config = SpeculativeConfig { draft_tokens: 4, ..Default::default() };

    let decoder = SpeculativeDecoder::new(draft, target, config);
    let result = decoder.generate(&[100], 4).await.unwrap();

    assert_eq!(result, vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn speculative_rejects_divergent_draft() {
    let draft = MockDraft { tokens: vec![1, 2, 3, 4] };
    let target = MockTarget {
        accept_count: 2,
        correction: Some(5),
        fallback_token: 99,
        eos: Some(0),
    };
    let config = SpeculativeConfig { draft_tokens: 4, ..Default::default() };

    let decoder = SpeculativeDecoder::new(draft, target, config);
    let result = decoder.generate(&[100], 3).await.unwrap();

    // Should accept 2 tokens plus correction
    assert_eq!(result, vec![1, 2, 5]);
}

#[tokio::test]
async fn speculative_fallback_on_empty_accept() {
    let draft = MockDraft { tokens: vec![1, 2, 3, 4] };
    let target = MockTarget {
        accept_count: 0,
        correction: None,
        fallback_token: 99,
        eos: Some(0),
    };
    let config = SpeculativeConfig { draft_tokens: 4, ..Default::default() };

    let decoder = SpeculativeDecoder::new(draft, target, config);
    let result = decoder.generate(&[100], 1).await.unwrap();

    // Should fall back to single token generation
    assert_eq!(result, vec![99]);
}

#[tokio::test]
async fn speculative_stops_at_eos() {
    let draft = MockDraft { tokens: vec![1, 2, 0] }; // 0 is EOS
    let target = MockTarget {
        accept_count: 3,
        correction: None,
        fallback_token: 99,
        eos: Some(0),
    };
    let config = SpeculativeConfig { draft_tokens: 4, ..Default::default() };

    let decoder = SpeculativeDecoder::new(draft, target, config);
    let result = decoder.generate(&[100], 10).await.unwrap();

    // Should stop after EOS
    assert_eq!(result, vec![1, 2, 0]);
}

#[tokio::test]
async fn speculative_config_draft_tokens() {
    let draft = MockDraft { tokens: vec![1, 2, 3, 4, 5, 6, 7, 8] };
    let target = MockTarget {
        accept_count: 2,
        correction: None,
        fallback_token: 99,
        eos: Some(0),
    };
    let config = SpeculativeConfig {
        draft_tokens: 2, // Only request 2 draft tokens
        ..Default::default()
    };

    let decoder = SpeculativeDecoder::new(draft, target, config);
    let result = decoder.generate(&[100], 2).await.unwrap();

    // Should only use 2 tokens per draft cycle
    assert_eq!(result, vec![1, 2]);
}

#[tokio::test]
async fn speculative_disabled_uses_standard() {
    let draft = MockDraft { tokens: vec![1, 2, 3, 4] };
    let target = MockTarget {
        accept_count: 4,
        correction: None,
        fallback_token: 42,
        eos: Some(0),
    };
    let config = SpeculativeConfig {
        enabled: false, // Disabled
        ..Default::default()
    };

    let decoder = SpeculativeDecoder::new(draft, target, config);
    let result = decoder.generate(&[100], 3).await.unwrap();

    // Should use standard generation (fallback token)
    assert_eq!(result, vec![42, 42, 42]);
}

#[test]
fn verify_result_accept_all() {
    let result = VerifyResult::accept_all(5);
    assert_eq!(result.accepted_count, 5);
    assert!(result.correction_token.is_none());
}

#[test]
fn verify_result_diverge_at() {
    let result = VerifyResult::diverge_at(3, 99);
    assert_eq!(result.accepted_count, 3);
    assert_eq!(result.correction_token, Some(99));
}

#[test]
fn speculative_config_default() {
    let config = SpeculativeConfig::default();
    assert_eq!(config.draft_tokens, 4);
    assert_eq!(config.acceptance_threshold, 0.9);
    assert!(config.enabled);
}
