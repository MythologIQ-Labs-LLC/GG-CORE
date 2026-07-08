//! CPU Flash Attention implementation.
//!
//! Tiled attention that computes softmax in blocks to reduce peak memory
//! from O(n^2) to O(n). Uses online softmax algorithm for numerical stability.

/// Configuration for Flash Attention.
#[derive(Debug, Clone)]
pub struct FlashAttnConfig {
    /// Tile size for blocked computation (default 64).
    pub block_size: usize,
    /// Dimension of each attention head.
    pub head_dim: usize,
}

impl Default for FlashAttnConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            head_dim: 64,
        }
    }
}

/// CPU Flash Attention with tiled computation.
#[derive(Debug)]
pub struct FlashAttn {
    config: FlashAttnConfig,
}

impl FlashAttn {
    /// Create a new Flash Attention instance.
    pub fn new(config: FlashAttnConfig) -> Self {
        Self { config }
    }

    /// Compute attention output using tiled algorithm.
    ///
    /// # Arguments
    /// * `query` - Query vector [head_dim]
    /// * `keys` - Key vectors flattened [seq_len * head_dim]
    /// * `values` - Value vectors flattened [seq_len * head_dim]
    /// * `seq_len` - Number of KV positions
    /// * `output` - Output buffer [head_dim]
    pub fn forward(
        &self,
        query: &[f32],
        keys: &[f32],
        values: &[f32],
        seq_len: usize,
        output: &mut [f32],
    ) {
        if seq_len == 0 {
            return;
        }

        let head_dim = self.config.head_dim;
        let block_size = self.config.block_size;

        // Online softmax state
        let mut global_max = f32::NEG_INFINITY;
        let mut global_sum = 0.0f32;

        // Accumulated weighted values
        let mut acc = vec![0.0f32; head_dim];

        // Pre-allocate buffer for scores to avoid allocation per block
        let mut scores_buf = vec![0.0f32; block_size];

        // Process in tiles
        let num_blocks = seq_len.div_ceil(block_size);

        for block_idx in 0..num_blocks {
            let start = block_idx * block_size;
            let end = (start + block_size).min(seq_len);
            let block_len = end - start;

            // Compute attention scores for this block
            let block_scores = &mut scores_buf[..block_len];
            let block_max = self.compute_block_scores(query, keys, start, block_len, block_scores);

            // Update global statistics and accumulate
            self.update_accumulator(
                block_scores,
                values,
                start,
                block_len,
                block_max,
                &mut global_max,
                &mut global_sum,
                &mut acc,
            );
        }

        // Normalize output
        if global_sum > 0.0 {
            for i in 0..head_dim.min(output.len()) {
                output[i] = acc[i] / global_sum;
            }
        }
    }

    /// Compute attention scores for a single block into the provided buffer.
    fn compute_block_scores(
        &self,
        query: &[f32],
        keys: &[f32],
        start: usize,
        block_len: usize,
        scores_out: &mut [f32],
    ) -> f32 {
        let head_dim = self.config.head_dim;
        let mut block_max = f32::NEG_INFINITY;

        for (i, score_slot) in scores_out.iter_mut().enumerate().take(block_len) {
            let key_offset = (start + i) * head_dim;
            let score = self.dot_product(query, &keys[key_offset..key_offset + head_dim]);
            *score_slot = score;
            block_max = block_max.max(score);
        }

        block_max
    }

    /// Update accumulator with block contribution using online softmax.
    #[allow(clippy::too_many_arguments)]
    fn update_accumulator(
        &self,
        scores: &[f32],
        values: &[f32],
        start: usize,
        _block_len: usize,
        block_max: f32,
        global_max: &mut f32,
        global_sum: &mut f32,
        acc: &mut [f32],
    ) {
        let head_dim = self.config.head_dim;

        // Compute correction factor if new max is higher
        let new_max = global_max.max(block_max);
        let correction = if *global_max > f32::NEG_INFINITY {
            (*global_max - new_max).exp()
        } else {
            0.0
        };

        // Scale existing accumulator by correction
        if correction > 0.0 && correction < 1.0 {
            for v in acc.iter_mut() {
                *v *= correction;
            }
            *global_sum *= correction;
        }

        // Add block contribution
        for (i, &score) in scores.iter().enumerate() {
            let weight = (score - new_max).exp();
            *global_sum += weight;

            let val_offset = (start + i) * head_dim;
            for j in 0..head_dim.min(acc.len()) {
                acc[j] += weight * values[val_offset + j];
            }
        }

        *global_max = new_max;
    }

    /// Compute dot product of two vectors.
    fn dot_product(&self, a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(&x, &y)| x * y).sum()
    }

    pub fn config(&self) -> &FlashAttnConfig {
        &self.config
    }
}
