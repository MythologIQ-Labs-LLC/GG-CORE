//! A CUDA build with no visible device must refuse offload, not run on CPU (#216).
//!
//! Meaningful only when the job hides every device (`CUDA_VISIBLE_DEVICES=-1`)
//! and sets `GG_EXPECT_NO_CUDA_DEVICE=1`. Otherwise it emits an explicit
//! NOT_QUALIFIED receipt instead of pretending this path was exercised.

#![cfg(feature = "gguf-cuda")]

#[path = "gguf_cuda_qual/common.rs"]
mod common;

use std::sync::Arc;

use common::{greedy, load, report, stream, ALL_LAYERS};

#[test]
fn cuda_build_without_a_visible_device_refuses_offload_but_serves_cpu() {
    if std::env::var("GG_EXPECT_NO_CUDA_DEVICE").as_deref() != Ok("1") {
        report(
            "hidden_device.qual_status",
            "NOT_QUALIFIED (opt-in missing)",
        );
        return;
    }
    assert_eq!(
        std::env::var("CUDA_VISIBLE_DEVICES").as_deref(),
        Ok("-1"),
        "the hidden-device gate must explicitly disable CUDA visibility"
    );
    let refused = load(ALL_LAYERS, 1024).err().unwrap_or_default();
    report("hidden_device.offload_error", format!("{refused:?}"));
    assert!(
        refused.contains("refusing CPU fallback"),
        "offload was accepted"
    );
    // Loading a model is not proof of serving. Actually generate CPU tokens
    // with CUDA hidden and verify the backend returns a usable stream.
    let cpu = Arc::new(load(0, 1024).expect("CPU load must still work"));
    let generated = stream(cpu, "Count from one to five in words.", greedy(32), None);
    report("hidden_device.cpu_generated_tokens", generated.tokens);
    report(
        "hidden_device.cpu_stream_result",
        format!("{:?}", generated.result),
    );
    assert!(
        generated.result.is_ok(),
        "CPU inference failed: {:?}",
        generated.result
    );
    assert!(
        generated.tokens > 0,
        "CPU generated no tokens with CUDA hidden"
    );
    report("hidden_device.qual_status", "PASS");
}
