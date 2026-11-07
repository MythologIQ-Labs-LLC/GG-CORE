//! Host physical-memory detection for default resource limits.
//!
//! The total-memory budget (which is also the Windows Job Object memory cap)
//! defaults to half of physical RAM, never below 2 GiB, so multi-gigabyte
//! models are admissible on real hosts while containment stays bounded.
//! An explicit `GG_CORE_MAX_TOTAL_MEMORY` always overrides it.

/// Floor for the default total-memory budget (the historical default).
pub const MIN_DEFAULT_TOTAL_MEMORY: usize = 2 * 1024 * 1024 * 1024;

/// Default total-memory budget: max(2 GiB, 50% of physical RAM). Falls back
/// to 2 GiB when physical RAM cannot be determined.
pub fn default_total_memory() -> usize {
    physical_memory_bytes()
        .map(|bytes| bytes / 2)
        .unwrap_or(0)
        .max(MIN_DEFAULT_TOTAL_MEMORY)
}

/// Installed physical memory in bytes, if the platform reports it.
#[cfg(windows)]
pub fn physical_memory_bytes() -> Option<usize> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is plain data; zeroed with dwLength set is the
    // documented valid input, and the pointer is valid for the call.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then(|| usize::try_from(status.ullTotalPhys).unwrap_or(usize::MAX))
}

/// Installed physical memory in bytes, if the platform reports it.
#[cfg(unix)]
pub fn physical_memory_bytes() -> Option<usize> {
    // SAFETY: sysconf has no memory-safety preconditions.
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || page_size <= 0 {
        return None;
    }
    (pages as usize).checked_mul(page_size as usize)
}

/// Installed physical memory in bytes, if the platform reports it.
#[cfg(not(any(windows, unix)))]
pub fn physical_memory_bytes() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_total_is_at_least_the_historical_floor() {
        assert!(default_total_memory() >= MIN_DEFAULT_TOTAL_MEMORY);
    }

    #[test]
    fn physical_memory_is_reported_on_supported_hosts() {
        let bytes = physical_memory_bytes().expect("host RAM must be detectable on CI/dev hosts");
        assert!(
            bytes >= 512 * 1024 * 1024,
            "implausible physical RAM: {bytes}"
        );
        assert_eq!(
            default_total_memory(),
            (bytes / 2).max(MIN_DEFAULT_TOTAL_MEMORY)
        );
    }
}
