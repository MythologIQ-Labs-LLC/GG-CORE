//! Fail-closed GGUF GPU layer offload (#216).
//!
//! llama.cpp quietly runs on the CPU when `n_gpu_layers > 0` but no GPU
//! device is registered: a CPU-only build, or a CUDA build on a host with
//! no usable device. The daemon's environment loader already refuses that
//! request; this check also covers Rust embedding and FFI callers that set
//! `GgufConfig::n_gpu_layers` directly. A zero request is never queried and
//! keeps the exact CPU-only behavior.

use llama_cpp_2::llama_backend::LlamaBackend;

use crate::engine::InferenceError;

/// Refuse a non-zero offload request when no GPU device is present.
pub(super) fn require_offload_device(
    n_gpu_layers: u32,
    device_present: bool,
) -> Result<(), InferenceError> {
    if n_gpu_layers == 0 || device_present {
        return Ok(());
    }
    Err(InferenceError::ModelError(format!(
        "GPU offload of {n_gpu_layers} layers requested, but llama.cpp has no GPU device; \
         refusing CPU fallback"
    )))
}

/// Check a load request against the process backend's registered devices.
pub(super) fn check(backend: &LlamaBackend, n_gpu_layers: u32) -> Result<(), InferenceError> {
    if n_gpu_layers == 0 {
        return Ok(());
    }
    require_offload_device(n_gpu_layers, backend.supports_gpu_offload())
}

#[cfg(test)]
#[path = "offload_tests.rs"]
mod tests;
