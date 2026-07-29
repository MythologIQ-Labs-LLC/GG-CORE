//! Seccomp-bpf syscall filtering for `UnixSandbox` (B-16 split from `unix.rs`).
//!
//! Restricts the syscalls available to the process to a minimal whitelist
//! required for inference. Relocated verbatim; behavior is unchanged.

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

/// Architecture identifier for aarch64 (kept for aarch64 filter parity)
#[cfg(target_os = "linux")]
#[allow(dead_code)]
const AUDIT_ARCH_AARCH64: u32 = 0xC00000B7;

/// seccomp_data structure for BPF filter (kernel ABI reference; the filter's
/// hardcoded offsets correspond to this layout even though it is never
/// constructed in userspace)
#[cfg(target_os = "linux")]
#[repr(C)]
#[allow(dead_code)]
struct SeccompData {
    nr: i32,
    arch: u32,
    instruction_pointer: u64,
    args: [u64; 6],
}

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
    /// Additional syscalls required for GPU (NVIDIA) driver access.
    /// Only included when `gpu_enabled` is true in `SandboxConfig`.
    #[cfg(target_os = "linux")]
    fn gpu_syscalls_x86_64() -> &'static [i32] {
        &[
            16,  // ioctl — NVIDIA kernel module communication
            9,   // mmap — GPU buffer mapping (already in base, harmless dup)
            10,  // mprotect — GPU memory protection changes
            25,  // mremap — GPU buffer resizing
            27,  // mincore — page residency check
            302, // prlimit64 — resource limit queries
        ]
    }

    /// Apply seccomp-bpf filter to restrict syscalls
    /// This provides defense-in-depth against code execution vulnerabilities
    #[cfg(target_os = "linux")]
    pub(super) fn apply_seccomp_filter(&self) -> Result<(), String> {
        // Syscall whitelist for inference operations (x86_64 numbers)
        // These are the minimal syscalls needed for the runtime
        const ALLOWED_SYSCALLS_X86_64: &[i32] = &[
            // File operations
            0,   // read
            1,   // write
            2,   // open
            3,   // close
            8,   // lseek
            9,   // mmap
            10,  // mprotect
            11,  // munmap
            12,  // brk
            16,  // ioctl
            22,  // pipe
            23,  // select
            24,  // sched_yield
            28,  // madvise
            257, // openat
            262, // newfstatat
            // Process management
            39,  // getpid
            60,  // exit
            186, // gettid
            218, // set_tid_address
            231, // exit_group
            // Signal handling
            13, // rt_sigaction
            14, // rt_sigprocmask
            15, // rt_sigreturn
            // Time
            35,  // nanosleep
            228, // clock_gettime
            229, // clock_getres
            // Thread operations
            56, // clone
            58, // fork
            59, // execve
            61, // wait4
            // IPC (for tokio)
            41, // socket
            42, // connect
            43, // accept
            44, // sendto
            45, // recvfrom
            46, // sendmsg
            47, // recvmsg
            53, // socketpair
            54, // setsockopt
            55, // getsockopt
            // Futex for synchronization
            202, // futex
            // Eventfd for tokio
            281, // eventfd2
            // Epoll for tokio
            232, // epoll_wait
            233, // epoll_ctl
            254, // epoll_create1
            // Random
            318, // getrandom
            // GPU driver support
            157, // prctl
            158, // arch_prctl
        ];

        // Build BPF filter program
        let mut filter = Vec::new();

        // Load architecture
        filter.push(SockFilter {
            code: bpf::LD | bpf_size::W | bpf_mode::ABS,
            jt: 0,
            jf: 0,
            k: 4, // offsetof(seccomp_data, arch)
        });

        // Check architecture (x86_64)
        filter.push(SockFilter {
            code: bpf::JMP | bpf_jmp::JEQ | bpf_src::K,
            jt: 0,
            jf: 4, // Skip to kill if wrong arch
            k: AUDIT_ARCH_X86_64,
        });

        // Load syscall number
        filter.push(SockFilter {
            code: bpf::LD | bpf_size::W | bpf_mode::ABS,
            jt: 0,
            jf: 0,
            k: 0, // offsetof(seccomp_data, nr)
        });

        // Conditionally add GPU driver syscalls
        if self.config.gpu_enabled {
            for &syscall_nr in Self::gpu_syscalls_x86_64() {
                filter.push(SockFilter {
                    code: bpf::JMP | bpf_jmp::JEQ | bpf_src::K,
                    jt: 1,
                    jf: 0,
                    k: syscall_nr as u32,
                });
            }
        }

        // Check against allowed syscalls
        for &syscall_nr in ALLOWED_SYSCALLS_X86_64 {
            filter.push(SockFilter {
                code: bpf::JMP | bpf_jmp::JEQ | bpf_src::K,
                jt: 1, // Jump to allow
                jf: 0, // Continue to next check
                k: syscall_nr as u32,
            });
        }

        // Default: kill process
        filter.push(SockFilter {
            code: bpf::RET | bpf_src::K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_KILL_PROCESS,
        });

        // Allow syscall
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

        // Apply seccomp filter using prctl
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

    #[cfg(not(target_os = "linux"))]
    pub(super) fn apply_seccomp_filter(&self) -> Result<(), String> {
        Ok(())
    }
}
