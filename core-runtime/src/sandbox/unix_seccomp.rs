//! Seccomp-bpf syscall filtering for `UnixSandbox` (B-16 split from `unix.rs`).
//!
//! Restricts the syscalls available to the process to an allowlist derived
//! empirically from the serving daemon (RC hardening, 2026-11-04): the daemon
//! was run under `strace -ff` across startup, every IPC surface (probes,
//! status, models list/load/unload, infer, streaming handshake) and shutdown,
//! and the filter allows that surface plus defensive vectored-IO/abort paths.
//! Unknown syscalls kill the process (`SECCOMP_RET_KILL_PROCESS`).
//!
//! Two earlier defects are fixed here (both found during RC review):
//! - the BPF jump offsets were constant (`jt: 1`), which only reached the
//!   ALLOW instruction from the final comparison — any earlier match fell
//!   through to other comparisons or the KILL itself, so the old filter
//!   killed the process on most allowed syscalls;
//! - the old list was hand-numbered with several mislabels (e.g. 281 noted
//!   as eventfd2 but actually epoll_pwait; 254 noted as epoll_create1 but
//!   actually inotify_add_watch) and lacked syscalls modern glibc/tokio
//!   require (accept4, fcntl, statx, clone3, rseq, eventfd2, epoll_create1,
//!   sigaltstack, bind, listen, ...). Syscall numbers now come from
//!   `libc::SYS_*` constants, which cannot be mislabeled.
//!
//! Deliberately NOT allowed (tightened versus the old list): `execve`,
//! `fork`/`vfork`, `connect` (the daemon only accepts; outbound sockets are
//! denied at the syscall layer, matching the C.O.R.E. Offline boundary) and
//! base `ioctl` (GPU mode adds it explicitly).

use super::UnixSandbox;

/// Seccomp filter flag for strict mode
#[cfg(target_os = "linux")]
const SECCOMP_MODE_FILTER: i32 = 2;

/// Seccomp return action: allow syscall
#[cfg(target_os = "linux")]
const SECCOMP_RET_ALLOW: u32 = 0x7FFF0000;

/// Seccomp return action: kill process
#[cfg(target_os = "linux")]
const SECCOMP_RET_KILL_PROCESS: u32 = 0x80000000;

// BPF opcode reference tables live in the `defs` child module (B-16 Razor split).
#[cfg(target_os = "linux")]
#[path = "unix_seccomp_defs.rs"]
mod defs;
#[cfg(target_os = "linux")]
use defs::{bpf, bpf_jmp, bpf_mode, bpf_size, bpf_src};

/// Architecture identifier for x86_64
#[cfg(target_os = "linux")]
const AUDIT_ARCH_X86_64: u32 = 0xC000003E;

/// BPF instruction structure
#[cfg(target_os = "linux")]
#[repr(C)]
struct SockFilter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

/// BPF program structure
#[cfg(target_os = "linux")]
#[repr(C)]
struct SockFprog {
    len: u16,
    filter: *const SockFilter,
}

impl UnixSandbox {
    /// Apply seccomp-bpf filter to restrict syscalls.
    /// This provides defense-in-depth against code execution vulnerabilities.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn apply_seccomp_filter(&self) -> Result<(), String> {
        let mut allowed = defs::allowed_syscalls();
        if self.config.gpu_enabled {
            allowed.extend(defs::gpu_syscalls());
        }
        let n = allowed.len();
        // jt/jf are u8 jump offsets; the allow landing pad sits n-i
        // instructions after comparison i (see layout below).
        if n > 250 {
            return Err(format!("seccomp allowlist too large for u8 jumps: {n}"));
        }

        // Layout: [load arch][arch check][load nr][cmp 0..n-1][KILL][ALLOW]
        let mut filter = Vec::with_capacity(n + 5);

        // Load architecture (offsetof(seccomp_data, arch) == 4)
        filter.push(SockFilter {
            code: bpf::LD | bpf_size::W | bpf_mode::ABS,
            jt: 0,
            jf: 0,
            k: 4,
        });
        // Wrong architecture -> jump straight to KILL, skipping
        // [load nr] + n comparisons = n + 1 instructions.
        filter.push(SockFilter {
            code: bpf::JMP | bpf_jmp::JEQ | bpf_src::K,
            jt: 0,
            jf: (n + 1) as u8,
            k: AUDIT_ARCH_X86_64,
        });
        // Load syscall number (offsetof(seccomp_data, nr) == 0)
        filter.push(SockFilter {
            code: bpf::LD | bpf_size::W | bpf_mode::ABS,
            jt: 0,
            jf: 0,
            k: 0,
        });
        // Comparison i has (n - 1 - i) comparisons and the KILL between it
        // and ALLOW: a match must skip (n - i) instructions.
        for (i, &nr) in allowed.iter().enumerate() {
            filter.push(SockFilter {
                code: bpf::JMP | bpf_jmp::JEQ | bpf_src::K,
                jt: (n - i) as u8,
                jf: 0,
                k: nr as u32,
            });
        }
        // Default: kill process
        filter.push(SockFilter {
            code: bpf::RET | bpf_src::K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_KILL_PROCESS,
        });
        // Allow landing pad
        filter.push(SockFilter {
            code: bpf::RET | bpf_src::K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        });

        let prog = SockFprog {
            len: filter.len() as u16,
            filter: filter.as_ptr(),
        };

        // PR_SET_NO_NEW_PRIVS = 38
        let result = unsafe { libc::prctl(38, 1, 0, 0, 0) };
        if result != 0 {
            return Err("Failed to set no_new_privs".to_string());
        }

        // PR_SET_SECCOMP = 22
        let result = unsafe { libc::prctl(22, SECCOMP_MODE_FILTER, &prog, 0, 0) };
        if result != 0 {
            return Err(format!(
                "Failed to apply seccomp filter: errno {}",
                std::io::Error::last_os_error()
            ));
        }

        Ok(())
    }

    /// Linux on a non-x86_64 architecture: the BPF allowlist above encodes
    /// x86_64 syscall numbers, so applying it elsewhere would kill the
    /// process on its first syscall. Fail loud instead of pretending.
    #[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
    pub(super) fn apply_seccomp_filter(&self) -> Result<(), String> {
        Err("seccomp allowlist is implemented for x86_64 only on this build".to_string())
    }

    /// Non-Linux unix (e.g. macOS): seccomp does not exist; the cgroup half
    /// of `apply` reports its own result. Kept Ok to preserve the library's
    /// long-standing behavior; the daemon boot layer decides platform policy.
    #[cfg(not(target_os = "linux"))]
    pub(super) fn apply_seccomp_filter(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    fn allowlist_fits_u8_jumps_with_gpu() {
        let n = defs::allowed_syscalls().len() + defs::gpu_syscalls().len();
        assert!(n <= 250, "allowlist too large: {n}");
    }

    #[test]
    fn allowlist_denies_exec_fork_connect_ioctl() {
        use libc::*;
        let allowed = defs::allowed_syscalls();
        for denied in [SYS_execve, SYS_fork, SYS_vfork, SYS_connect, SYS_ioctl] {
            assert!(
                !allowed.contains(&denied),
                "syscall {denied} must be denied"
            );
        }
    }
}
