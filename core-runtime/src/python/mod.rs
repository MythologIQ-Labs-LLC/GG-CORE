// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! PyO3 Python bindings for GG-CORE (text-based v1 API)

pub mod exceptions;
pub mod inference;
pub mod models;
pub mod runtime;
pub mod session;
pub mod streaming;

use pyo3::prelude::*;

/// GG-CORE Python module
#[pymodule]
fn gg_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<runtime::Runtime>()?;
    m.add_class::<session::Session>()?;
    m.add_class::<session::AsyncSession>()?;
    m.add_class::<inference::InferenceParams>()?;
    m.add_class::<inference::InferenceResult>()?;
    m.add_class::<models::ModelInfo>()?;
    m.add_class::<streaming::StreamingResult>()?;
    m.add_class::<streaming::StreamingIterator>()?;
    Ok(())
}
