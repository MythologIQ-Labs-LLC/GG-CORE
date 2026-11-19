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
/// Refusal must occur on the first inference context, not at weight load.
const OVERSIZED_CTX: u32 = 4_000_000;

#[test]
fn oversized_gpu_context_is_refused_then_a_normal_load_works() {
    report("oom.vram_before_mib", format!("{:?}", vram_mib()));
    // Loading the model itself is not the dangerous allocation. The KV and
    // compute buffers are committed on first inference context creation, so
    // actually request inference and require GG-CORE's pre-allocation guard.
    let oversized = Arc::new(load(ALL_LAYERS, OVERSIZED_CTX).expect("GPU model weights load"));
    let attempt = stream(Arc::clone(&oversized), "Hello", greedy(4), None);
    match attempt.result {
        Err(ref error) if error.contains("GPU context admission refused before allocation") => {
            report(
                "oom.outcome",
                "GPU context rejected before llama.cpp allocation",
            );
            report("oom.refusal", "PASS");
        }
        other => {
            report("oom.refusal", "FAIL (no proven pre-allocation refusal)");
            panic!("oversized context was not refused by GPU admission: {other:?}");
        }
    }
    drop(oversized);
    report("oom.vram_after_mib", format!("{:?}", vram_mib()));
    let normal = Arc::new(load(ALL_LAYERS, 1024).expect("normal load after oom"));
    let run = stream(normal, "Hello", greedy(8), None);
    assert!(
        run.result.is_ok() && run.tokens > 0,
        "unusable after oom: {:?}",
        run.result
    );
}
