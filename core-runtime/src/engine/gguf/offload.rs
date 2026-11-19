//! Fail-closed GGUF GPU layer offload (#216).
//!
//! llama.cpp quietly runs on the CPU when `n_gpu_layers > 0` but no GPU
//! device is registered: a CPU-only build, or a CUDA build on a host with
//! no usable device. The daemon's environment loader already refuses that
//! request; this check also covers Rust embedding and FFI callers that set
//! `GgufConfig::n_gpu_layers` directly. A zero request is never queried and
//! keeps the exact CPU-only behavior.

use llama_cpp_2::{
    list_llama_ggml_backend_devices, llama_backend::LlamaBackend, LlamaBackendDeviceType,
};

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

const GPU_MIN_HEADROOM: usize = 256 * 1024 * 1024;
const GPU_HEADROOM_FRACTION: usize = 10;

/// Check GPU context demand before llama.cpp creates the KV/compute buffers.
///
/// The default llama.cpp device list can select any visible GPU. Until the
/// selected model's device mapping is separately attested, conservatively
/// require the full working set to fit on *each* eligible GPU. This also
/// refuses drivers reporting unknown or inconsistent dedicated-memory data.
/// This is a pre-allocation safety gate, not a promise that external processes
/// cannot change free memory between sampling and allocation.
pub(super) fn check_context_memory(required_bytes: usize) -> Result<(), InferenceError> {
    let eligible: Vec<_> = list_llama_ggml_backend_devices()
        .into_iter()
        .filter(|dev| {
            matches!(
                dev.device_type,
                LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu
            )
        })
        .map(|dev| (dev.memory_free, dev.memory_total))
        .collect();
    admit_gpu_working_set(required_bytes, &eligible)
}

/// Pure admission predicate, shared by native preflight and model-free tests.
///
/// Reserve the larger of 256 MiB and 10% of reported device capacity for
/// driver/runtime allocations. Missing, overflowing and oversubscribed
/// information refuses before any native context allocation. Caller-side
/// serialisation covers GG-CORE context creation races within this process.
fn admit_gpu_working_set(
    required_bytes: usize,
    device_memory: &[(usize, usize)],
) -> Result<(), InferenceError> {
    if required_bytes == 0 || required_bytes == usize::MAX || device_memory.is_empty() {
        return Err(InferenceError::ModelError(
            "GPU context admission refused: absent GPU capacity or unbounded working set".into(),
        ));
    }
    for &(free_bytes, total_bytes) in device_memory {
        let reserve = (total_bytes / GPU_HEADROOM_FRACTION).max(GPU_MIN_HEADROOM);
        let within_capacity = total_bytes > 0
            && free_bytes > 0
            && free_bytes <= total_bytes
            && required_bytes
                .checked_add(reserve)
                .is_some_and(|need| need <= free_bytes);
        if !within_capacity {
            return Err(InferenceError::ModelError(format!(
                "GPU context admission refused before allocation: estimated {required_bytes} bytes,                  device free {free_bytes} bytes, total {total_bytes} bytes;                  refusing driver-managed host-memory fallback"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "offload_tests.rs"]
mod tests;
