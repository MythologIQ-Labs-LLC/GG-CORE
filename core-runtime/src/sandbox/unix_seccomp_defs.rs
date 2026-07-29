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
