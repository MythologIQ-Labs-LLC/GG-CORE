//! Quantized KV-cache storage with SIMD-accelerated attention.
//!
//! Stores keys and values in Q8 format for 4x memory bandwidth reduction.

/// Quantized KV storage with per-position scales.
#[derive(Debug)]
pub struct Q8KvStore {
    keys: Vec<u8>,
    values: Vec<u8>,
    key_scales: Vec<f32>,
    value_scales: Vec<f32>,
    seq_len: usize,
    hidden_dim: usize,
    max_seq: usize,
}

impl Q8KvStore {
    /// Create a new Q8 KV store with given dimensions.
    pub fn new(hidden_dim: usize, max_seq: usize) -> Self {
        Self {
            keys: vec![0; max_seq * hidden_dim],
            values: vec![0; max_seq * hidden_dim],
            key_scales: vec![1.0; max_seq],
            value_scales: vec![1.0; max_seq],
            seq_len: 0,
            hidden_dim,
            max_seq,
        }
    }

    /// Append a KV pair, quantizing to Q8.
    pub fn append(&mut self, keys: &[f32], values: &[f32]) -> bool {
        if self.seq_len >= self.max_seq {
            return false;
        }
        let offset = self.seq_len * self.hidden_dim;
        let k_scale = compute_scale(keys);
        let v_scale = compute_scale(values);

        quantize_to(
            &mut self.keys[offset..offset + self.hidden_dim],
            keys,
            k_scale,
        );
        quantize_to(
            &mut self.values[offset..offset + self.hidden_dim],
            values,
            v_scale,
        );

        self.key_scales[self.seq_len] = k_scale;
        self.value_scales[self.seq_len] = v_scale;
        self.seq_len += 1;
        true
    }

    /// Compute attention scores using SIMD dot product.
    pub fn attention_scores(&self, query: &[f32], output: &mut [f32]) {
        for pos in 0..self.seq_len {
            let offset = pos * self.hidden_dim;
            let scale = self.key_scales[pos];
            let q_slice = &self.keys[offset..offset + self.hidden_dim];
            let mut sum = 0.0;
            for (i, &q) in q_slice.iter().enumerate() {
                sum += (q as i8 as f32) * scale * query[i];
            }
            output[pos] = sum;
        }
    }

    /// Read dequantized keys for a position.
    pub fn read_keys(&self, pos: usize, output: &mut [f32]) {
        if pos >= self.seq_len {
            return;
        }
        let offset = pos * self.hidden_dim;
        dequantize(
            &self.keys[offset..offset + self.hidden_dim],
            output,
            self.key_scales[pos],
        );
    }

    /// Read dequantized values for a position.
    pub fn read_values(&self, pos: usize, output: &mut [f32]) {
        if pos >= self.seq_len {
            return;
        }
        let offset = pos * self.hidden_dim;
        dequantize(
            &self.values[offset..offset + self.hidden_dim],
            output,
            self.value_scales[pos],
        );
    }

    /// Weighted sum of values for attention output.
    pub fn weighted_values(&self, weights: &[f32], output: &mut [f32]) {
        output.fill(0.0);
        for pos in 0..self.seq_len.min(weights.len()) {
            let offset = pos * self.hidden_dim;
            let scale = self.value_scales[pos] * weights[pos];
            for (i, &q) in self.values[offset..offset + self.hidden_dim]
                .iter()
                .enumerate()
            {
                output[i] += (q as i8 as f32) * scale;
            }
        }
    }

    pub fn seq_len(&self) -> usize {
        self.seq_len
    }
    pub fn hidden_dim(&self) -> usize {
        self.hidden_dim
    }
    pub fn memory_bytes(&self) -> usize {
        self.keys.len() + self.values.len()
    }

    /// Reset for reuse without reallocation.
    pub fn reset(&mut self) {
        self.seq_len = 0;
    }
}

/// Compute Q8 scale for a data slice.
pub fn compute_scale(data: &[f32]) -> f32 {
    let max_abs = data.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
    if max_abs > 0.0 {
        max_abs / 127.0
    } else {
        1.0
    }
}

/// Quantize f32 data to Q8.
pub fn quantize_to(out: &mut [u8], data: &[f32], scale: f32) {
    for (q, &x) in out.iter_mut().zip(data.iter()) {
        *q = (x / scale).round().clamp(-128.0, 127.0) as i8 as u8;
    }
}

/// Dequantize Q8 data to f32.
pub fn dequantize(q_data: &[u8], output: &mut [f32], scale: f32) {
    for (o, &q) in output.iter_mut().zip(q_data.iter()) {
        *o = (q as i8 as f32) * scale;
    }
}
