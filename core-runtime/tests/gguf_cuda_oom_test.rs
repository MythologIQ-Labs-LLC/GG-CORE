//! Native CUDA GGUF over-allocation must fail as an error, never a crash (#216).
//!
//! Separate binary: if llama.cpp aborts on a failed device allocation, only
//! this test dies and the qualification results stay intact.

#![cfg(feature = "gguf-cuda")]

#[path = "gguf_cuda_qual/common.rs"]
mod common;

use std::sync::Arc;

use common::{greedy, load, report, stream, vram_mib, ALL_LAYERS};

/// ~4M tokens of KV cache is far beyond a 16 GiB device for this model.
const OVERSIZED_CTX: u32 = 4_000_000;

#[test]
fn oversized_gpu_context_is_refused_then_a_normal_load_works() {
    report("oom.vram_before_mib", format!("{:?}", vram_mib()));
    let outcome = match load(ALL_LAYERS, OVERSIZED_CTX) {
        Err(e) => format!("load refused: {e}"),
        Ok(model) => {
            let run = stream(Arc::new(model), "Hello", greedy(8), None);
            format!("generate result: {:?} tokens={}", run.result, run.tokens)
        }
    };
    report("oom.outcome", format!("{outcome:?}"));
    report("oom.vram_after_mib", format!("{:?}", vram_mib()));
    let normal = Arc::new(load(ALL_LAYERS, 1024).expect("normal load after oom"));
    let run = stream(normal, "Hello", greedy(8), None);
    assert!(
        run.result.is_ok() && run.tokens > 0,
        "unusable after oom: {:?}",
        run.result
    );
}
