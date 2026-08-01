//! Quantization support with layer-level kernel dispatch.
//!
//! Implements Q4_0 and Q8_0 formats with per-matmul kernel selection.
//! Uses SIMD-accelerated kernels from simd_matmul for Q8/Q4 dot products.

use super::simd_matmul;

/// Block size for quantized formats.
pub const QUANT_BLOCK_SIZE: usize = 32;

/// Supported quantization formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantFormat {
    /// Full precision (no quantization).
    F32,
    /// 8-bit symmetric quantization.
    Q8_0,
    /// 4-bit symmetric quantization.
    Q4_0,
}

impl QuantFormat {
    /// Bytes per block for this format.
    pub fn bytes_per_block(&self) -> usize {
        match self {
            Self::F32 => QUANT_BLOCK_SIZE * 4,
            Self::Q8_0 => QUANT_BLOCK_SIZE + 4, // 32 bytes + 1 scale
            Self::Q4_0 => QUANT_BLOCK_SIZE / 2 + 4, // 16 bytes + 1 scale
        }
    }

    /// Memory reduction factor vs F32.
    pub fn compression_ratio(&self) -> f32 {
        match self {
            Self::F32 => 1.0,
            Self::Q8_0 => 4.0,
            Self::Q4_0 => 8.0,
        }
    }
}

/// Quantized tensor with format-specific storage.
#[derive(Debug, Clone)]
pub struct QuantizedTensor {
    format: QuantFormat,
    data: Vec<u8>,
    scales: Vec<f32>,
    shape: [usize; 2],
}

impl QuantizedTensor {
    /// Create from F32 data with specified format.
    pub fn from_f32(data: &[f32], rows: usize, cols: usize, format: QuantFormat) -> Self {
        match format {
            QuantFormat::F32 => Self::from_f32_identity(data, rows, cols),
            QuantFormat::Q8_0 => Self::quantize_q8(data, rows, cols),
            QuantFormat::Q4_0 => Self::quantize_q4(data, rows, cols),
        }
    }

    /// Layer-level matmul dispatch - selects kernel based on format.
    pub fn matmul(&self, input: &[f32], output: &mut [f32]) {
        match self.format {
            QuantFormat::F32 => self.matmul_f32(input, output),
            QuantFormat::Q8_0 => self.matmul_q8(input, output),
            QuantFormat::Q4_0 => self.matmul_q4(input, output),
        }
    }

    pub fn format(&self) -> QuantFormat {
        self.format
    }
    pub fn shape(&self) -> [usize; 2] {
        self.shape
    }

    fn from_f32_identity(data: &[f32], rows: usize, cols: usize) -> Self {
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        Self {
            format: QuantFormat::F32,
            data: bytes,
            scales: vec![],
            shape: [rows, cols],
        }
    }

    fn quantize_q8(data: &[f32], rows: usize, cols: usize) -> Self {
        let num_blocks = data.len().div_ceil(QUANT_BLOCK_SIZE);
        let mut quantized = Vec::with_capacity(num_blocks * QUANT_BLOCK_SIZE);
        let mut scales = Vec::with_capacity(num_blocks);

        for block in data.chunks(QUANT_BLOCK_SIZE) {
            let max_abs = block.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            let scale = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
            scales.push(scale);
            for &val in block {
                let q = (val / scale).round().clamp(-128.0, 127.0) as i8;
                quantized.push(q as u8);
            }
            quantized.resize(quantized.len() + (QUANT_BLOCK_SIZE - block.len()), 0);
        }
        Self {
            format: QuantFormat::Q8_0,
            data: quantized,
            scales,
            shape: [rows, cols],
        }
    }

    fn quantize_q4(data: &[f32], rows: usize, cols: usize) -> Self {
        let num_blocks = data.len().div_ceil(QUANT_BLOCK_SIZE);
        let mut quantized = Vec::with_capacity(num_blocks * QUANT_BLOCK_SIZE / 2);
        let mut scales = Vec::with_capacity(num_blocks);

        for block in data.chunks(QUANT_BLOCK_SIZE) {
            let max_abs = block.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            let scale = if max_abs > 0.0 { max_abs / 7.0 } else { 1.0 };
            scales.push(scale);

            let mut padded = [0.0f32; QUANT_BLOCK_SIZE];
            padded[..block.len()].copy_from_slice(block);

            for i in 0..QUANT_BLOCK_SIZE / 2 {
                let q0 = ((padded[i * 2] / scale).round().clamp(-8.0, 7.0) as i8 + 8) as u8;
                let q1 = ((padded[i * 2 + 1] / scale).round().clamp(-8.0, 7.0) as i8 + 8) as u8;
                quantized.push((q1 << 4) | (q0 & 0x0F));
            }
        }
        Self {
            format: QuantFormat::Q4_0,
            data: quantized,
            scales,
            shape: [rows, cols],
        }
    }

    // Range loops: `i`/`j` index output/input AND drive `(i*cols+j)*4` byte offsets,
    // so the index is load-bearing (not a needless range loop).
    #[allow(clippy::needless_range_loop)]
    fn matmul_f32(&self, input: &[f32], output: &mut [f32]) {
        let [rows, cols] = self.shape;
        for i in 0..rows {
            let mut sum = 0.0f32;
            for j in 0..cols.min(input.len()) {
                let idx = (i * cols + j) * 4;
                let w = f32::from_le_bytes([
                    self.data[idx],
                    self.data[idx + 1],
                    self.data[idx + 2],
                    self.data[idx + 3],
                ]);
                sum += w * input[j];
            }
            output[i] = sum;
        }
    }

    // `i` indexes output AND drives `i*blocks_per_row` scale/data offsets — load-bearing.
    #[allow(clippy::needless_range_loop)]
    fn matmul_q8(&self, input: &[f32], output: &mut [f32]) {
        let [rows, cols] = self.shape;
        let blocks_per_row = cols.div_ceil(QUANT_BLOCK_SIZE);

        for i in 0..rows {
            let mut sum = 0.0f32;
            for b in 0..blocks_per_row {
                let block_start = b * QUANT_BLOCK_SIZE;
                let block_end = (block_start + QUANT_BLOCK_SIZE).min(cols).min(input.len());
                let block_len = block_end.saturating_sub(block_start);

                if block_len == 0 {
                    continue;
                }

                let scale = self.scales[i * blocks_per_row + b];
                let data_offset = (i * blocks_per_row + b) * QUANT_BLOCK_SIZE;
                let q_slice = &self.data[data_offset..data_offset + block_len];
                let in_slice = &input[block_start..block_end];

                sum += simd_matmul::dot_q8(q_slice, in_slice, scale);
            }
            output[i] = sum;
        }
    }

    // `i` indexes output AND drives `i*blocks_per_row` scale/data offsets — load-bearing.
    #[allow(clippy::needless_range_loop)]
    fn matmul_q4(&self, input: &[f32], output: &mut [f32]) {
        let [rows, cols] = self.shape;
        let blocks_per_row = cols.div_ceil(QUANT_BLOCK_SIZE);

        for i in 0..rows {
            let mut sum = 0.0f32;
            for b in 0..blocks_per_row {
                let block_start = b * QUANT_BLOCK_SIZE;
                let block_end = (block_start + QUANT_BLOCK_SIZE).min(cols).min(input.len());
                let block_len = block_end.saturating_sub(block_start);
                let packed_len = block_len.div_ceil(2);

                if packed_len == 0 {
                    continue;
                }

                let scale = self.scales[i * blocks_per_row + b];
                let data_offset = (i * blocks_per_row + b) * (QUANT_BLOCK_SIZE / 2);
                let q_slice = &self.data[data_offset..data_offset + packed_len];
                let in_slice = &input[block_start..block_end];

                sum += simd_matmul::dot_q4(q_slice, in_slice, scale);
            }
            output[i] = sum;
        }
    }
}
