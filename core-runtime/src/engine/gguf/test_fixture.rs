//! Shared real-model fixture gate for GGUF tests (N7 successor, audit V3).
//!
//! Without the real Qwen2.5-0.5B fixture (for example an LFS pointer in a
//! source-only checkout) real-model tests skip. With
//! `GG_CORE_REQUIRE_REAL_MODEL=1`, as set by the qualification jobs, a
//! missing fixture is a test failure instead of a silent pass.

use std::path::Path;

/// Path of the real fixture relative to `core-runtime/`.
pub(super) const FIXTURE: &str = "../models/qwen2.5-0.5b-instruct-q4_k_m.gguf";

/// Whether skipped real-model tests must fail instead.
pub(super) fn required() -> bool {
    std::env::var("GG_CORE_REQUIRE_REAL_MODEL").as_deref() == Ok("1")
}

/// The real fixture, or `None` to skip. Panics when the fixture is missing
/// and [`required`] is set.
pub(super) fn real_model() -> Option<&'static Path> {
    let path = Path::new(FIXTURE);
    let mut magic = [0u8; 4];
    let is_gguf = std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .is_ok()
        && &magic == b"GGUF";
    if is_gguf {
        return Some(path);
    }
    assert!(
        !required(),
        "GG_CORE_REQUIRE_REAL_MODEL=1 but no real GGUF at {path:?}"
    );
    eprintln!("SKIP: real GGUF fixture not present at {path:?}");
    None
}
