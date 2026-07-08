//! Enhanced SIMD-accelerated tokenizer for fast text processing.
//!
//! Provides AVX2/NEON-accelerated operations including:
//! - Whitespace detection
//! - Character class detection
//! - BPE merge operations
//! - Token boundary detection

use std::collections::HashMap;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// Token vocabulary entry.
#[derive(Debug, Clone)]
struct TokenEntry {
    bytes: Vec<u8>,
    _id: u32,
    _score: f32, // For BPE merge priority
}

/// SIMD-accelerated tokenizer for fast text processing.
pub struct SimdTokenizer {
    vocab: Vec<TokenEntry>,
    vocab_map: HashMap<Vec<u8>, u32>,
    eos_token: u32,
    bos_token: u32,
    /// Precomputed merge scores for BPE
    merge_scores: HashMap<(u32, u32), f32>,
}

/// Statistics for tokenizer performance.
#[derive(Debug, Default, Clone)]
pub struct TokenizerStats {
    pub total_tokens_encoded: u64,
    pub total_tokens_decoded: u64,
    pub total_bytes_processed: u64,
    pub simd_operations: u64,
    pub scalar_fallbacks: u64,
}

impl SimdTokenizer {
    /// Create tokenizer from vocabulary bytes.
    /// Format: one token per line, format: "token_bytes\tscore"
    pub fn from_vocab(
        vocab_bytes: &[u8],
        eos_token: u32,
        bos_token: u32,
    ) -> Result<Self, TokenizerError> {
        let mut vocab = Vec::new();
        let mut vocab_map = HashMap::new();

        for (id, line) in vocab_bytes.split(|&b| b == b'\n').enumerate() {
            if line.is_empty() {
                continue;
            }

            // Parse line: "token_bytes\tscore" or just "token_bytes"
            let (token_bytes, score) = if let Some(tab_pos) = line.iter().position(|&b| b == b'\t')
            {
                let token = line[..tab_pos].to_vec();
                let score_str = String::from_utf8_lossy(&line[tab_pos + 1..]);
                let score = score_str.parse().unwrap_or(0.0f32);
                (token, score)
            } else {
                (line.to_vec(), 0.0f32)
            };

            vocab_map.insert(token_bytes.clone(), id as u32);
            vocab.push(TokenEntry {
                bytes: token_bytes,
                _id: id as u32,
                _score: score,
            });
        }

        Ok(Self {
            vocab,
            vocab_map,
            eos_token,
            bos_token,
            merge_scores: HashMap::new(),
        })
    }

    /// Create tokenizer with merge rules for BPE.
    pub fn with_merges(
        vocab_bytes: &[u8],
        merges: &[(u32, u32, f32)],
        eos_token: u32,
        bos_token: u32,
    ) -> Result<Self, TokenizerError> {
        let mut tokenizer = Self::from_vocab(vocab_bytes, eos_token, bos_token)?;

        for &(t1, t2, score) in merges {
            tokenizer.merge_scores.insert((t1, t2), score);
        }

        Ok(tokenizer)
    }

    /// Find whitespace positions using AVX2 SIMD (x86_64 only).
    ///
    /// # Safety
    /// Caller must verify AVX2 support via `is_x86_feature_detected!("avx2")`.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn find_whitespace_avx2(text: &[u8]) -> Vec<usize> {
        let mut positions = Vec::new();
        let space = _mm256_set1_epi8(b' ' as i8);
        let newline = _mm256_set1_epi8(b'\n' as i8);
        let tab = _mm256_set1_epi8(b'\t' as i8);
        let carriage = _mm256_set1_epi8(b'\r' as i8);

        let chunks = text.chunks_exact(32);
        let remainder = chunks.remainder();

        for (chunk_idx, chunk) in chunks.enumerate() {
            let data = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);
            let is_space = _mm256_cmpeq_epi8(data, space);
            let is_newline = _mm256_cmpeq_epi8(data, newline);
            let is_tab = _mm256_cmpeq_epi8(data, tab);
            let is_carriage = _mm256_cmpeq_epi8(data, carriage);

            let is_ws = _mm256_or_si256(
                _mm256_or_si256(is_space, is_newline),
                _mm256_or_si256(is_tab, is_carriage),
            );
            let mask = _mm256_movemask_epi8(is_ws) as u32;
            let base = chunk_idx * 32;
            Self::extract_positions(mask, base, &mut positions);
        }

        Self::find_whitespace_remainder(remainder, text.len(), &mut positions);
        positions
    }

    /// Find whitespace positions using NEON SIMD (aarch64 only).
    ///
    /// # Safety
    /// Caller must be on aarch64 where NEON is always available.
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn find_whitespace_neon(text: &[u8]) -> Vec<usize> {
        let mut positions = Vec::new();

        let chunks = text.chunks_exact(16);
        let remainder = chunks.remainder();

        for (chunk_idx, chunk) in chunks.enumerate() {
            let data = vld1q_u8(chunk.as_ptr());
            let space = vdupq_n_u8(b' ');
            let newline = vdupq_n_u8(b'\n');
            let tab = vdupq_n_u8(b'\t');

            let is_space = vceqq_u8(data, space);
            let is_newline = vceqq_u8(data, newline);
            let is_tab = vceqq_u8(data, tab);

            let is_ws = vorrq_u8(vorrq_u8(is_space, is_newline), is_tab);
            let mask = vaddvq_u8(vshrq_n_u8(is_ws, 7)) as u16;

            let base = chunk_idx * 16;
            for bit in 0..16 {
                if (mask >> bit) & 1 == 1 {
                    positions.push(base + bit);
                }
            }
        }

        Self::find_whitespace_remainder(remainder, text.len(), &mut positions);
        positions
    }

    /// Extract bit positions from mask (helper for SIMD).
    #[cfg(target_arch = "x86_64")]
    fn extract_positions(mask: u32, base: usize, positions: &mut Vec<usize>) {
        for bit in 0..32 {
            if (mask >> bit) & 1 == 1 {
                positions.push(base + bit);
            }
        }
    }

    /// Handle remainder bytes after SIMD chunks.
    fn find_whitespace_remainder(remainder: &[u8], total_len: usize, positions: &mut Vec<usize>) {
        let base = total_len - remainder.len();
        for (i, &b) in remainder.iter().enumerate() {
            if b == b' ' || b == b'\n' || b == b'\t' || b == b'\r' {
                positions.push(base + i);
            }
        }
    }

    /// Find whitespace positions (scalar fallback).
    fn find_whitespace_scalar(text: &[u8]) -> Vec<usize> {
        text.iter()
            .enumerate()
            .filter(|(_, &b)| b == b' ' || b == b'\n' || b == b'\t' || b == b'\r')
            .map(|(i, _)| i)
            .collect()
    }

    /// Find whitespace with automatic SIMD dispatch.
    pub fn find_whitespace(text: &[u8]) -> Vec<usize> {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") {
                // SAFETY: AVX2 feature is detected before calling
                return unsafe { Self::find_whitespace_avx2(text) };
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            // SAFETY: NEON is always available on aarch64
            return unsafe { Self::find_whitespace_neon(text) };
        }

        Self::find_whitespace_scalar(text)
    }

    /// Find token boundaries using SIMD-accelerated detection.
    ///
    /// # Safety
    /// Caller must verify AVX2 support via `is_x86_feature_detected!("avx2")`.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn find_token_boundaries_avx2(text: &[u8]) -> Vec<usize> {
        let mut positions = Vec::new();

        // Detect common token boundary characters
        let space = _mm256_set1_epi8(b' ' as i8);
        let newline = _mm256_set1_epi8(b'\n' as i8);
        let tab = _mm256_set1_epi8(b'\t' as i8);
        let punct_start = _mm256_set1_epi8(0x21); // '!'
        let punct_end = _mm256_set1_epi8(0x40); // '@'

        let chunks = text.chunks_exact(32);
        let remainder = chunks.remainder();

        for (chunk_idx, chunk) in chunks.enumerate() {
            let data = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);

            // Check for whitespace
            let is_space = _mm256_cmpeq_epi8(data, space);
            let is_newline = _mm256_cmpeq_epi8(data, newline);
            let is_tab = _mm256_cmpeq_epi8(data, tab);
            let is_ws = _mm256_or_si256(_mm256_or_si256(is_space, is_newline), is_tab);

            // Check for punctuation (simplified)
            let is_punct = _mm256_and_si256(
                _mm256_cmpgt_epi8(data, punct_start),
                _mm256_cmpgt_epi8(punct_end, data),
            );

            let is_boundary = _mm256_or_si256(is_ws, is_punct);
            let mask = _mm256_movemask_epi8(is_boundary) as u32;
            let base = chunk_idx * 32;
            Self::extract_positions(mask, base, &mut positions);
        }

        // Handle remainder
        let base = text.len() - remainder.len();
        for (i, &b) in remainder.iter().enumerate() {
            if b.is_ascii_whitespace() || b.is_ascii_punctuation() {
                positions.push(base + i);
            }
        }

        positions
    }

    /// Find token boundaries with automatic SIMD dispatch.
    pub fn find_token_boundaries(text: &[u8]) -> Vec<usize> {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") {
                return unsafe { Self::find_token_boundaries_avx2(text) };
            }
        }

        // Scalar fallback
        text.iter()
            .enumerate()
            .filter(|(_, &b)| b.is_ascii_whitespace() || b.is_ascii_punctuation())
            .map(|(i, _)| i)
            .collect()
    }

    /// Encode text to token IDs using greedy BPE.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let bytes = text.as_bytes();
        let mut tokens = Vec::with_capacity(bytes.len() / 4);
        let mut pos = 0;

        while pos < bytes.len() {
            let (best_len, best_id) = self.find_longest_match(bytes, pos);
            tokens.push(best_id);
            pos += best_len;
        }

        tokens
    }

    /// Encode text with BPE merge operations.
    pub fn encode_with_merges(&self, text: &str) -> Vec<u32> {
        let bytes = text.as_bytes();

        // Start with byte-level tokens
        let mut tokens: Vec<u32> = bytes.iter().map(|&b| b as u32).collect();

        // Apply BPE merges iteratively
        loop {
            let mut best_merge: Option<(usize, u32, f32)> = None;

            for i in 0..tokens.len().saturating_sub(1) {
                let pair = (tokens[i], tokens[i + 1]);
                if let Some(&score) = self.merge_scores.get(&pair) {
                    if best_merge.map_or(true, |(_, _, s)| score > s) {
                        best_merge = Some((i, pair.0, score)); // Use first token as merged ID
                    }
                }
            }

            if let Some((pos, merged_id, _)) = best_merge {
                tokens[pos] = merged_id;
                tokens.remove(pos + 1);
            } else {
                break;
            }
        }

        tokens
    }

    /// Find longest matching token from position.
    fn find_longest_match(&self, bytes: &[u8], pos: usize) -> (usize, u32) {
        let max_len = (bytes.len() - pos).min(16);

        for len in (1..=max_len).rev() {
            if let Some(&id) = self.vocab_map.get(&bytes[pos..pos + len]) {
                return (len, id);
            }
        }

        // Fallback to single byte
        (1, bytes[pos] as u32)
    }

    /// Decode token IDs to text.
    pub fn decode(&self, tokens: &[u32]) -> Result<String, TokenizerError> {
        let mut bytes = Vec::new();
        for &id in tokens {
            if let Some(entry) = self.vocab.get(id as usize) {
                bytes.extend_from_slice(&entry.bytes);
            } else {
                return Err(TokenizerError::InvalidToken(id));
            }
        }
        String::from_utf8(bytes).map_err(|e| TokenizerError::DecodingFailed(e.to_string()))
    }

    /// Get end-of-sequence token ID.
    pub fn eos_token(&self) -> u32 {
        self.eos_token
    }

    /// Get beginning-of-sequence token ID.
    pub fn bos_token(&self) -> u32 {
        self.bos_token
    }

    /// Get vocabulary size.
    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    /// Count tokens in text without full encoding.
    pub fn count_tokens(&self, text: &str) -> usize {
        let bytes = text.as_bytes();
        let mut count = 0;
        let mut pos = 0;

        while pos < bytes.len() {
            let (len, _) = self.find_longest_match(bytes, pos);
            count += 1;
            pos += len;
        }

        count
    }

    /// Check if a token is a special token.
    pub fn is_special_token(&self, id: u32) -> bool {
        id == self.eos_token || id == self.bos_token
    }
}

/// Errors for tokenizer operations.
#[derive(Debug, thiserror::Error)]
pub enum TokenizerError {
    #[error("Invalid token ID: {0}")]
    InvalidToken(u32),

    #[error("Decoding failed: {0}")]
    DecodingFailed(String),

    #[error("Invalid vocabulary format: {0}")]
    InvalidVocab(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_whitespace() {
        let text = b"hello world\tnew\nline";
        let positions = SimdTokenizer::find_whitespace(text.as_slice());

        assert!(positions.contains(&5)); // space
        assert!(positions.contains(&11)); // tab
        assert!(positions.contains(&15)); // newline
    }

    #[test]
    fn test_find_whitespace_empty() {
        let text = b"nospaceshere";
        let positions = SimdTokenizer::find_whitespace(text.as_slice());
        assert!(positions.is_empty());
    }

    #[test]
    fn test_find_token_boundaries() {
        let text = b"hello, world!";
        let positions = SimdTokenizer::find_token_boundaries(text.as_slice());

        assert!(positions.contains(&5)); // comma
        assert!(positions.contains(&6)); // space
        assert!(positions.contains(&12)); // exclamation
    }

    #[test]
    fn test_simple_vocab() {
        let vocab = b"hello\nworld\ntest\n";
        let tokenizer = SimdTokenizer::from_vocab(vocab.as_slice(), 3, 4).unwrap();

        assert_eq!(tokenizer.vocab_size(), 3);
        assert_eq!(tokenizer.eos_token(), 3);
        assert_eq!(tokenizer.bos_token(), 4);
    }

    #[test]
    fn test_encode_decode() {
        let vocab = b"hello\nworld\n \n";
        let tokenizer = SimdTokenizer::from_vocab(vocab.as_slice(), 3, 4).unwrap();

        let encoded = tokenizer.encode("hello world");
        assert!(!encoded.is_empty());

        let decoded = tokenizer.decode(&encoded).unwrap();
        assert_eq!(decoded, "hello world");
    }

    #[test]
    fn test_count_tokens() {
        let vocab = b"hello\nworld\ntest\n";
        let tokenizer = SimdTokenizer::from_vocab(vocab.as_slice(), 3, 4).unwrap();

        // Each unknown byte becomes a single token
        let count = tokenizer.count_tokens("hello");
        assert_eq!(count, 1); // "hello" is in vocab
    }
}
