// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Python Session classes for inference operations (text-based v1 API)

use std::sync::Arc;

use pyo3::prelude::*;
use tokio::runtime::Runtime as TokioRuntime;

use super::exceptions::AuthenticationError;
use super::inference::{InferenceParams, InferenceResult};
use crate::engine::InferenceParams as RustParams;
use crate::ipc::SessionToken;
use crate::scheduler::Priority;
use crate::Runtime as CoreRuntime;

/// Synchronous session for inference operations
///
/// Use as a context manager:
/// ```python
/// with runtime.session() as session:
///     result = session.infer("model-id", "Hello world")
/// ```
#[pyclass]
pub struct Session {
    runtime: Arc<CoreRuntime>,
    tokio: Arc<TokioRuntime>,
    token: SessionToken,
    valid: bool,
}

impl Session {
    pub(super) fn new(
        runtime: Arc<CoreRuntime>,
        tokio: Arc<TokioRuntime>,
        token: SessionToken,
    ) -> Self {
        Self { runtime, tokio, token, valid: true }
    }

    fn check_valid(&self) -> PyResult<()> {
        if !self.valid {
            return Err(AuthenticationError::new_err("session has been closed"));
        }
        self.tokio.block_on(async {
            self.runtime.ipc_handler.auth.validate(&self.token).await
        }).map_err(|e| AuthenticationError::new_err(e.to_string()))
    }
}

#[pymethods]
impl Session {
    /// Run inference on a model (text-based)
    ///
    /// Args:
    ///     model_id: Model identifier string
    ///     prompt: Text prompt for generation
    ///     params: Optional inference parameters
    ///
    /// Returns:
    ///     InferenceResult with generated text
    #[pyo3(signature = (model_id, prompt, params=None))]
    fn infer(
        &self,
        model_id: &str,
        prompt: &str,
        params: Option<&InferenceParams>,
    ) -> PyResult<InferenceResult> {
        self.check_valid()?;

        let rust_params = params
            .map(RustParams::from)
            .unwrap_or_default();

        let result = self.tokio.block_on(async {
            let (_id, rx) = self.runtime.request_queue
                .enqueue_with_response(
                    model_id.to_string(),
                    prompt.to_string(),
                    rust_params,
                    Priority::Normal,
                )
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

            rx.await
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("worker dropped channel"))?
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))
        })?;

        Ok(InferenceResult::from(result))
    }

    /// Load a model via lifecycle coordinator
    ///
    /// Args:
    ///     model_path: Path to model file (relative to base_path)
    ///
    /// Returns:
    ///     Handle ID of the loaded model
    fn load_model(&self, model_path: &str) -> PyResult<u64> {
        self.check_valid()?;

        let validated = self.runtime.model_loader.validate_path(model_path)?;
        let metadata = self.runtime.model_loader.load_metadata(&validated)?;
        let model_id = metadata.name.clone();

        let model = crate::engine::gguf::load_gguf_model(
            validated.as_path(),
            &model_id,
            &crate::engine::gguf::GgufConfig::default(),
        )?;

        let handle = self.tokio.block_on(async {
            self.runtime.model_lifecycle.load(model_id, metadata, model).await
        }).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;

        Ok(handle.id())
    }

    /// Unload a model via lifecycle coordinator
    fn unload_model(&self, model_id: &str) -> PyResult<()> {
        self.check_valid()?;

        self.tokio.block_on(async {
            self.runtime.model_lifecycle.unload(model_id).await
        }).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;

        Ok(())
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &mut self,
        _exc_type: Option<PyObject>,
        _exc_val: Option<PyObject>,
        _exc_tb: Option<PyObject>,
    ) -> bool {
        self.valid = false;
        false
    }
}

/// Async session for asyncio-based inference
///
/// Use with async context manager:
/// ```python
/// async with await runtime.session_async() as session:
///     result = await session.infer("model-id", "Hello")
/// ```
#[pyclass]
pub struct AsyncSession {
    runtime: Arc<CoreRuntime>,
    token: SessionToken,
    valid: bool,
}

impl AsyncSession {
    pub(super) fn new(runtime: Arc<CoreRuntime>, token: SessionToken) -> Self {
        Self { runtime, token, valid: true }
    }
}

#[pymethods]
impl AsyncSession {
    /// Async inference (text-based)
    #[pyo3(signature = (model_id, prompt, params=None))]
    fn infer<'py>(
        &self,
        py: Python<'py>,
        model_id: String,
        prompt: String,
        params: Option<InferenceParams>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if !self.valid {
            return Err(AuthenticationError::new_err("session closed"));
        }

        let runtime = self.runtime.clone();
        let token = self.token.clone();
        let rust_params = params
            .as_ref()
            .map(RustParams::from)
            .unwrap_or_default();

        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            runtime.ipc_handler.auth.validate(&token).await
                .map_err(|e| AuthenticationError::new_err(e.to_string()))?;

            let (_id, rx) = runtime
                .request_queue
                .enqueue_with_response(
                    model_id,
                    prompt,
                    rust_params,
                    Priority::Normal,
                )
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

            let result = rx.await
                .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("worker dropped channel"))?
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))?;

            Ok(InferenceResult::from(result))
        })
    }

    fn __aenter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __aexit__(
        &mut self,
        _exc_type: Option<PyObject>,
        _exc_val: Option<PyObject>,
        _exc_tb: Option<PyObject>,
    ) -> bool {
        self.valid = false;
        false
    }
}
