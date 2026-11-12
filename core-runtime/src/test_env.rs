//! Process-wide lock for library tests that mutate `GG_CORE_*` variables.
//!
//! One lock shared by every module (#182 audit V3), recovered on poison so a
//! failed test cannot cascade into the others.

use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Hold this for the whole duration of any env-mutating test.
pub(crate) fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
