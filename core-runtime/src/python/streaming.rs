// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Python streaming types for text-based output

use pyo3::prelude::*;

/// A single streaming result chunk (text-based)
#[pyclass]
#[derive(Clone)]
pub struct StreamingResult {
    /// Generated text for this chunk
    #[pyo3(get)]
    pub text: String,

    /// Whether this is the final chunk
    #[pyo3(get)]
    pub is_final: bool,

    /// Error message if generation failed (None if successful)
    #[pyo3(get)]
    pub error: Option<String>,
}

#[pymethods]
impl StreamingResult {
    fn __repr__(&self) -> String {
        if let Some(ref err) = self.error {
            format!("StreamingResult(error={})", err)
        } else {
            format!(
                "StreamingResult(text='{}...', is_final={})",
                &self.text[..self.text.len().min(20)],
                self.is_final
            )
        }
    }

    #[getter]
    fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Iterator for streaming inference results (text-based)
///
/// Yields StreamingResult objects. Currently returns the full
/// output as a single chunk (true streaming is a future feature).
#[pyclass]
pub struct StreamingIterator {
    text: Option<String>,
    done: bool,
}

impl StreamingIterator {
    pub fn new(text: String) -> Self {
        Self { text: Some(text), done: false }
    }
}

#[pymethods]
impl StreamingIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<StreamingResult> {
        if slf.done {
            return None;
        }
        slf.done = true;
        Some(StreamingResult {
            text: slf.text.take().unwrap_or_default(),
            is_final: true,
            error: None,
        })
    }
}

/// Async iterator for streaming inference (future implementation)
#[pyclass]
pub struct AsyncStreamingIterator {
    text: Option<String>,
    done: bool,
}

impl AsyncStreamingIterator {
    #[allow(dead_code)]
    pub fn new(text: String) -> Self {
        Self { text: Some(text), done: false }
    }
}

#[pymethods]
impl AsyncStreamingIterator {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__(mut slf: PyRefMut<'_, Self>) -> Option<StreamingResult> {
        if slf.done {
            return None;
        }
        slf.done = true;
        Some(StreamingResult {
            text: slf.text.take().unwrap_or_default(),
            is_final: true,
            error: None,
        })
    }
}
