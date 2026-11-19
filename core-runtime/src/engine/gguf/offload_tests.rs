//! Offload refusal tests (#216).

use super::require_offload_device;
use crate::engine::InferenceError;

#[test]
fn zero_layers_never_requires_a_device() {
    assert!(require_offload_device(0, false).is_ok());
    assert!(require_offload_device(0, true).is_ok());
}

#[test]
fn nonzero_layers_without_a_device_are_refused() {
    for layers in [1, 33, i32::MAX as u32] {
        match require_offload_device(layers, false) {
            Err(InferenceError::ModelError(m)) => assert!(m.contains("refusing CPU fallback")),
            other => panic!("expected refusal for {layers}, got {other:?}"),
        }
    }
}

#[test]
fn nonzero_layers_with_a_device_are_allowed() {
    assert!(require_offload_device(33, true).is_ok());
}

/// A CPU-only build must refuse an in-process offload request before any
/// model file is opened, rather than silently running on the CPU.
#[cfg(not(any(feature = "gguf-cuda", feature = "gguf-metal")))]
#[test]
fn cpu_only_build_refuses_direct_offload_before_loading() {
    let config = super::super::GgufConfig {
        n_ctx: 512,
        n_threads: 1,
        n_gpu_layers: 1,
    };
    let missing = std::path::Path::new("no-such-dir/never-opened.gguf");
    match super::super::backend::LlamaBackendInner::load(missing, &config) {
        Err(InferenceError::ModelError(m)) => {
            assert!(m.contains("refusing CPU fallback"), "{m}")
        }
        Err(other) => panic!("wrong refusal: {other:?}"),
        Ok(_) => panic!("CPU-only build accepted GPU offload"),
    }
}

const MIB: usize = 1024 * 1024;
const GIB: usize = 1024 * MIB;

#[test]
fn gpu_context_preflight_refuses_driver_paging_before_allocation() {
    // The previously observed 4M-context case requested ~46,875 MiB of KV
    // on a 16 GiB CUDA card, with the driver paging into system RAM.
    let required = 46_875 * MIB;
    let device = [(15 * GIB, 16 * GIB)];
    assert!(super::admit_gpu_working_set(required, &device).is_err());
}

#[test]
fn gpu_preflight_reserves_driver_headroom_but_accepts_bounded_context() {
    assert!(super::admit_gpu_working_set(GIB, &[(12 * GIB, 16 * GIB)]).is_ok());
    assert!(super::admit_gpu_working_set(12 * GIB, &[(12 * GIB, 16 * GIB)]).is_err());
}

#[test]
fn gpu_preflight_refuses_missing_invalid_or_overflowed_device_readings() {
    for snapshot in [
        vec![],
        vec![(0, 16 * GIB)],
        vec![(16 * GIB, 0)],
        vec![(17 * GIB, 16 * GIB)],
    ] {
        assert!(super::admit_gpu_working_set(GIB, &snapshot).is_err());
    }
    assert!(super::admit_gpu_working_set(0, &[(15 * GIB, 16 * GIB)]).is_err());
    assert!(super::admit_gpu_working_set(usize::MAX, &[(15 * GIB, 16 * GIB)]).is_err());
}

#[test]
fn gpu_preflight_requires_all_eligible_devices_to_fit_until_pinned() {
    assert!(super::admit_gpu_working_set(GIB, &[(12 * GIB, 16 * GIB), (2 * GIB, 8 * GIB)]).is_ok());
    assert!(super::admit_gpu_working_set(3 * GIB, &[(12 * GIB, 16 * GIB), (2 * GIB, 8 * GIB)]).is_err());
}

#[test]
fn cpu_model_does_not_invoke_gpu_memory_gate() {
    // The backend calls GPU preflight only when shape.gpu_offload is true.
    let cpu = super::super::working_memory::ContextShape::new(4096);
    assert!(!cpu.gpu_offload);
}
