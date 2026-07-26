// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Result and parameter marshaling helpers for the inference FFI surface.

use std::ffi::CString;

use super::types::{CoreInferenceParams, CoreInferenceResult};
use crate::engine::InferenceParams;

/// Convert C params to Rust params
pub(super) fn params_from_c(c: &CoreInferenceParams) -> InferenceParams {
    InferenceParams {
        max_tokens: c.max_tokens as usize,
        temperature: c.temperature,
        top_p: c.top_p,
        top_k: c.top_k as usize,
        stream: c.stream,
        timeout_ms: if c.timeout_ms == 0 {
            None
        } else {
            Some(c.timeout_ms)
        },
    }
}

/// Write inference result to C struct
pub(super) fn write_inference_result(
    result: &crate::engine::InferenceResult,
    out: &mut CoreInferenceResult,
) {
    let cstr = CString::new(result.output.clone()).unwrap_or_default();
    out.output_text = cstr.into_raw();
    out.tokens_generated = result.tokens_generated as u32;
    out.finished = result.finished;
}

impl Clone for CoreInferenceParams {
    fn clone(&self) -> Self {
        Self {
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stream: self.stream,
            timeout_ms: self.timeout_ms,
        }
    }
}
