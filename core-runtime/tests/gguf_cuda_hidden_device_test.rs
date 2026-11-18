//! A CUDA build with no visible device must refuse offload, not run on CPU (#216).
//!
//! Meaningful only when the job hides every device (`CUDA_VISIBLE_DEVICES=-1`)
//! and sets `GG_EXPECT_NO_CUDA_DEVICE=1`; otherwise it returns immediately.

#![cfg(feature = "gguf-cuda")]

#[path = "gguf_cuda_qual/common.rs"]
mod common;

use common::{load, report, ALL_LAYERS};

#[test]
fn cuda_build_without_a_visible_device_refuses_offload_but_serves_cpu() {
    if std::env::var("GG_EXPECT_NO_CUDA_DEVICE").as_deref() != Ok("1") {
        return;
    }
    let refused = load(ALL_LAYERS, 1024).err().unwrap_or_default();
    report("hidden_device.offload_error", format!("{refused:?}"));
    assert!(
        refused.contains("refusing CPU fallback"),
        "offload was accepted"
    );
    assert!(load(0, 1024).is_ok(), "CPU load must still work");
}
