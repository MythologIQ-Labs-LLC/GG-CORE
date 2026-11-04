//! BPF opcode reference tables for the seccomp filter (B-16 split from
//! `unix_seccomp.rs`). Pure `pub const` data; behavior is unchanged.

/// BPF instruction classes
///
/// Deliberately complete opcode reference table kept for seccomp filter
/// maintenance; unused entries are retained intentionally.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(super) mod bpf {
    pub const LD: u16 = 0x00;
    pub const LDX: u16 = 0x01;
    pub const ST: u16 = 0x02;
    pub const STX: u16 = 0x03;
    pub const ALU: u16 = 0x04;
    pub const JMP: u16 = 0x05;
    pub const RET: u16 = 0x06;
    pub const MISC: u16 = 0x07;
}

/// BPF size modifiers (complete reference table; see `bpf`)
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(super) mod bpf_size {
    pub const W: u16 = 0x00;
    pub const H: u16 = 0x08;
    pub const B: u16 = 0x10;
    pub const DW: u16 = 0x18;
}

/// BPF mode modifiers (complete reference table; see `bpf`)
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(super) mod bpf_mode {
    pub const IMM: u16 = 0x00;
    pub const ABS: u16 = 0x20;
    pub const IND: u16 = 0x40;
    pub const MEM: u16 = 0x60;
    pub const LEN: u16 = 0x80;
    pub const MSH: u16 = 0xA0;
}

/// BPF source modifiers (complete reference table; see `bpf`)
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(super) mod bpf_src {
    pub const K: u16 = 0x00;
    pub const X: u16 = 0x08;
}

/// BPF jump conditions (complete reference table; see `bpf`)
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub(super) mod bpf_jmp {
    pub const JA: u16 = 0x00;
    pub const JEQ: u16 = 0x10;
    pub const JGT: u16 = 0x20;
    pub const JGE: u16 = 0x30;
    pub const JSET: u16 = 0x40;
}

/// Syscall allowlist for the serving daemon, derived empirically (strace of
/// `serve` across startup, every IPC surface and shutdown; RC 2026-11-04)
/// plus defensive vectored-IO/abort paths. `libc::SYS_*` constants cannot be
/// mislabeled. Deliberately absent: execve, fork/vfork, connect, base ioctl.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) fn allowed_syscalls() -> Vec<libc::c_long> {
    use libc::*;
    vec![
        // File IO (model load, socket file, config)
        SYS_read,
        SYS_write,
        SYS_pread64,
        SYS_readv,
        SYS_writev,
        SYS_lseek,
        SYS_close,
        SYS_open,
        SYS_openat,
        SYS_access,
        SYS_unlink,
        SYS_fcntl,
        SYS_poll,
        SYS_fstat,
        SYS_newfstatat,
        SYS_statx,
        // Memory
        SYS_mmap,
        SYS_mprotect,
        SYS_munmap,
        SYS_mremap,
        SYS_brk,
        SYS_madvise,
        // Sockets: accept-side only — no SYS_connect (Offline boundary)
        SYS_socket,
        SYS_bind,
        SYS_listen,
        SYS_accept,
        SYS_accept4,
        SYS_shutdown,
        SYS_sendto,
        SYS_recvfrom,
        SYS_sendmsg,
        SYS_recvmsg,
        SYS_socketpair,
        SYS_setsockopt,
        SYS_getsockopt,
        // Threads & sync (tokio workers; clone3+rseq are modern glibc thread spawn)
        SYS_clone,
        SYS_clone3,
        SYS_futex,
        SYS_sched_yield,
        SYS_sched_getaffinity,
        SYS_set_robust_list,
        SYS_rseq,
        SYS_gettid,
        SYS_set_tid_address,
        SYS_prctl,
        SYS_arch_prctl,
        SYS_sigaltstack,
        SYS_membarrier,
        // Signals (runtime + panic/abort paths)
        SYS_rt_sigaction,
        SYS_rt_sigprocmask,
        SYS_rt_sigreturn,
        SYS_restart_syscall,
        SYS_tgkill,
        // Event loop (mio/tokio)
        SYS_epoll_create1,
        SYS_epoll_ctl,
        SYS_epoll_wait,
        SYS_epoll_pwait,
        SYS_eventfd2,
        // Time
        SYS_nanosleep,
        SYS_clock_nanosleep,
        SYS_clock_gettime,
        SYS_clock_getres,
        // Process info / limits / entropy
        SYS_getpid,
        SYS_exit,
        SYS_exit_group,
        SYS_prlimit64,
        SYS_getrandom,
    ]
}

/// Additional syscalls required for GPU (NVIDIA) driver access.
/// Only included when `gpu_enabled` is true in `SandboxConfig`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) fn gpu_syscalls() -> Vec<libc::c_long> {
    use libc::*;
    vec![SYS_ioctl, SYS_mincore]
}
