// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Python inference parameter and result types (text-based v1 API)

use pyo3::prelude::*;

use crate::engine::InferenceParams as RustParams;
use crate::engine::InferenceResult as RustResult;

/// Inference parameters for controlling generation
///
/// Example:
/// ```python
/// params = InferenceParams(max_tokens=256, temperature=0.7)
/// result = session.infer("model-id", "Hello world", params)
/// ```
#[pyclass]
#[derive(Clone)]
pub struct InferenceParams {
    #[pyo3(get, set)]
    pub max_tokens: u32,

    #[pyo3(get, set)]
    pub temperature: f32,

    #[pyo3(get, set)]
    pub top_p: f32,

    #[pyo3(get, set)]
    pub top_k: u32,

    #[pyo3(get, set)]
    pub stream: bool,

    #[pyo3(get, set)]
    pub timeout_ms: Option<u64>,
}

#[pymethods]
impl InferenceParams {
    #[new]
    #[pyo3(signature = (max_tokens=256, temperature=0.7, top_p=0.9, top_k=40, stream=false, timeout_ms=None))]
    fn new(
        max_tokens: u32,
        temperature: f32,
        top_p: f32,
        top_k: u32,
        stream: bool,
        timeout_ms: Option<u64>,
    ) -> Self {
        Self { max_tokens, temperature, top_p, top_k, stream, timeout_ms }
    }

    fn __repr__(&self) -> String {
        format!(
            "InferenceParams(max_tokens={}, temperature={}, top_p={}, top_k={})",
            self.max_tokens, self.temperature, self.top_p, self.top_k
        )
    }
}

impl Default for InferenceParams {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            stream: false,
            timeout_ms: None,
        }
    }
}

impl From<&InferenceParams> for RustParams {
    fn from(py: &InferenceParams) -> Self {
        Self {
            max_tokens: py.max_tokens as usize,
            temperature: py.temperature,
            top_p: py.top_p,
            top_k: py.top_k as usize,
            stream: py.stream,
            timeout_ms: py.timeout_ms,
        }
    }
}

/// Result from inference operation (text-based)
#[pyclass]
#[derive(Clone)]
pub struct InferenceResult {
    /// Generated text output
    #[pyo3(get)]
    pub output: String,

    /// Number of tokens generated
    #[pyo3(get)]
    pub tokens_generated: usize,

    /// Whether generation finished normally
    #[pyo3(get)]
    pub finished: bool,
}

#[pymethods]
impl InferenceResult {
    fn __repr__(&self) -> String {
        format!(
            "InferenceResult(tokens_generated={}, finished={})",
            self.tokens_generated, self.finished
        )
    }

    fn __len__(&self) -> usize {
        self.tokens_generated
    }

    fn __str__(&self) -> &str {
        &self.output
    }
}

impl From<RustResult> for InferenceResult {
    fn from(result: RustResult) -> Self {
        Self {
            output: result.output,
            tokens_generated: result.tokens_generated,
            finished: result.finished,
        }
    }
}
