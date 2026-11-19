//! Native CUDA GGUF over-allocation must be refused on the pinned 16 GiB
//! acceptance device (#216). This is a targeted qualification control, not
//! a universal memory cap for every possible accelerator.
//!
//! Separate binary: if llama.cpp aborts on a failed device allocation, only
//! this test dies and the qualification results stay intact.

#![cfg(feature = "gguf-cuda")]

#[path = "gguf_cuda_qual/common.rs"]
mod common;

use std::sync::Arc;

use common::{greedy, load, report, stream, vram_mib, ALL_LAYERS};

/// ~4M tokens of KV cache is far beyond a 16 GiB device for this model.
/// Admission must refuse before registering the model or allocating a context.
const OVERSIZED_CTX: u32 = 4_000_000;

#[test]
fn oversized_gpu_context_is_refused_then_a_normal_load_works() {
    report("oom.vram_before_mib", format!("{:?}", vram_mib()));
    // Loading weights alone may fit, but the configured context must also
    // fit before a model can be registered as serving-ready. Enforce this
    // before the first user request instead of waiting for inference.
    match load(ALL_LAYERS, OVERSIZED_CTX) {
        Err(error) => {
            assert!(
                error.contains("GPU context admission refused before allocation"),
                "wrong refusal reason, not VRAM preflight: {error}"
            );
            report("oom.outcome", "model load rejected on GPU context preflight");
            report("oom.refusal", "PASS");
        }
        Ok(model) => {
            drop(model);
            report("oom.refusal", "FAIL (unusable model was admitted)");
            panic!("GPU model admitted a context that exceeds device capacity");
        }
    }
    report("oom.vram_after_mib", format!("{:?}", vram_mib()));
    let normal = Arc::new(load(ALL_LAYERS, 1024).expect("normal load after oom"));
    let run = stream(normal, "Hello", greedy(8), None);
    assert!(
        run.result.is_ok() && run.tokens > 0,
        "unusable after oom: {:?}",
        run.result
    );
}
