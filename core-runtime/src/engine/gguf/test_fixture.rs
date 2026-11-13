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

static REAL_MODEL_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Hold for the whole of a timing-sensitive or CPU-loading real-model test
/// so CPU load from one never overlaps another's timing (review finding 2).
pub(super) fn serial() -> std::sync::MutexGuard<'static, ()> {
    REAL_MODEL_SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Stops the spinner threads when dropped, also when the test panics.
struct Spinners {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Drop for Spinners {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.threads.drain(..).for_each(|t| drop(t.join()));
    }
}

/// Run `f` on this thread while 8 other threads saturate the CPU.
pub(super) fn under_load<T>(f: impl FnOnce() -> T) -> T {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let threads = (0..8)
        .map(|_| {
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    x = std::hint::black_box(
                        x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
                    );
                }
            })
        })
        .collect();
    let _spinners = Spinners { stop, threads };
    f()
}
