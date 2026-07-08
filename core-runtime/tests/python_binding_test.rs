// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! F-40 python-bindings test binding (FEATURE_INDEX).
//!
//! Exercises the conversion seam between the PyO3 surface and the engine
//! without embedding a Python interpreter. Compile-gated on the `python`
//! feature; run with `--features python` where a Python toolchain exists.

#![cfg(feature = "python")]

use gg_core::engine::{InferenceParams as RustParams, InferenceResult as RustResult};
use gg_core::python::inference::{InferenceParams, InferenceResult};

#[test]
fn default_params_convert_losslessly() {
    let py_params = InferenceParams::default();

    // Documented defaults on the Python surface.
    assert_eq!(py_params.max_tokens, 256);
    assert_eq!(py_params.temperature, 0.7);
    assert_eq!(py_params.top_p, 0.9);
    assert_eq!(py_params.top_k, 40);
    assert!(!py_params.stream);
    assert_eq!(py_params.timeout_ms, None);

    // Every field survives the seam, including the u32 -> usize widenings.
    let rust_params = RustParams::from(&py_params);
    assert_eq!(rust_params.max_tokens, 256usize);
    assert_eq!(rust_params.temperature, 0.7);
    assert_eq!(rust_params.top_p, 0.9);
    assert_eq!(rust_params.top_k, 40usize);
    assert!(!rust_params.stream);
    assert_eq!(rust_params.timeout_ms, None);
}

#[test]
fn result_roundtrip_preserves_output() {
    let rust_result = RustResult {
        output: "greatest good".to_string(),
        tokens_generated: 2,
        finished: true,
    };

    let py_result = InferenceResult::from(rust_result);
    assert_eq!(py_result.output, "greatest good");
    assert_eq!(py_result.tokens_generated, 2);
    assert!(py_result.finished);
}
