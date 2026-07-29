# Research Brief — B-16: `sandbox/unix.rs` Section 4 Razor Refactor

**Date**: 2026-07-28
**Analyst**: The Qor-logic Analyst
**Target**: B-16 (audit 2026-07-08 R2, P3) — `core-runtime/src/sandbox/unix.rs` is 535
lines, over the Section 4 Razor 250-line limit. Split into ≤250-line files without
changing behavior.
**Scope**: the file's internal structure and clean, behavior-preserving split seams.
**Security note**: this is sandbox / seccomp-BPF / cgroup code — the refactor must be a
**byte-identical relocation** of logic, verified by the existing sandbox + sandbox-escape
security suites.

---

## Executive Summary

`unix.rs` (535 lines) has three cohesive, separable concerns behind one `UnixSandbox`
type: **seccomp-BPF syscall filtering** (the bulk), **cgroup v2 resource limits**, and the
**core struct + `Sandbox` trait impl**. Rust allows a type's `impl` to span multiple files
via child modules that retain access to private fields, so each concern can move to a
sibling file as an `impl UnixSandbox` block with **no logic change** — the exact pattern
used for `inference_streaming.rs` / `inference_degraded.rs`. Splitting seccomp and cgroup
out, plus the inline tests, brings `unix.rs` to ~150 lines and every sibling under 250.
Correctness is gated by the existing `sandbox_test` + `security_sandbox_escape_test`
suites, which must stay green.

## Findings (verified)

### F1 — three cohesive concerns behind one type
- **Seccomp machinery** (the bulk, ~230 lines): `SECCOMP_MODE_FILTER`/`SECCOMP_RET_*`
  consts (`:33-41`), `bpf`/`bpf_size`/`bpf_mode`/`bpf_src`/`bpf_jmp` submodules (`:49-102`),
  `AUDIT_ARCH_*` consts (`:103-108`), `SeccompData`/`SockFilter`/`SockFprog` structs
  (`:116-141`), and the impl methods `gpu_syscalls_x86_64` (`:221`) + `apply_seccomp_filter`
  (both the `#[cfg(target_os="linux")]` form `:235` and the non-linux stub `:391`).
- **Cgroup machinery** (~60 lines): `CGROUP_V2_BASE`/`SANDBOX_CGROUP_NAME` consts (`:26-29`)
  + impl methods `cgroups_v2_available` (`:159`) and `apply_cgroup_limits` (`:166`).
- **Core**: `pub struct UnixSandbox` (`:142`), `new` (`:150`), and `impl Sandbox for
  UnixSandbox` (`:396` — `apply`/`is_active`/`get_usage`). `apply` orchestrates
  `apply_cgroup_limits` + `apply_seccomp_filter`.

### F2 — the impl can span files (child-module pattern, in-repo precedent)
- `impl UnixSandbox` methods can be split into sibling files declared as child modules of
  `unix` (`#[path="unix_seccomp.rs"] mod seccomp;` etc.). A child module accesses the
  parent type's private fields (`config`, `cgroup_path`) and private consts, so the moved
  methods compile unchanged. Precedent: `engine/inference_streaming.rs` and
  `engine/inference_degraded.rs` (this session) do exactly this. Methods called from
  `unix.rs`'s `apply` (`apply_seccomp_filter`, `apply_cgroup_limits`) become `pub(super)`
  so the parent can still call them.

### F3 — inline tests are ~44 lines and externalizable
- `#[cfg(test)] mod tests` (`:491-535`) can move to `sandbox/unix_tests.rs` via
  `#[cfg(test)] #[path="unix_tests.rs"] mod tests;`, mirroring `onnx/classifier_tests.rs`.

### F4 — the security suites are the behavior-preservation gate
- `core-runtime/tests/sandbox_test.rs` and `core-runtime/tests/security_sandbox_escape_test.rs`
  exercise the sandbox surface. A pure relocation must leave both fully green; that is the
  refactor's acceptance signal (no new tests needed — the split adds no behavior).

## Blueprint Alignment

| Claim (backlog B-16) | Actual finding | Status |
|---|---|---|
| `unix.rs` > 250 (Razor debt) | 535 lines | CONFIRMED |
| Fix via `/qor-refactor` under L3 audit | Clean 3-way split by concern; child-module impls | MATCH (approach ready) |
| Behavior must not change | Relocation only; existing security suites gate it | MATCH (test-gated) |

## Recommendations (scope forks for the plan)

1. **Single bounded L3 refactor cycle** — split by concern into child-module `impl` files:
   - `sandbox/unix_seccomp.rs` — seccomp consts + `bpf*` submodules +
     `SeccompData`/`SockFilter`/`SockFprog` + `gpu_syscalls_x86_64` + `apply_seccomp_filter`
     (both cfg forms). ~230 lines.
   - `sandbox/unix_cgroup.rs` — cgroup consts + `cgroups_v2_available` + `apply_cgroup_limits`.
     ~65 lines.
   - `sandbox/unix_tests.rs` — the inline tests via `#[path]`. ~45 lines.
   - `unix.rs` retains the struct, `new`, `impl Sandbox`, and the three `mod` declarations.
     ~150 lines. Every file ≤ 250.
2. **Strictly mechanical**: move lines verbatim; the only edits are (a) `mod` declarations
   in `unix.rs`, (b) `pub(super)` on the two methods `apply` calls across the module
   boundary, (c) `use super::*;` / targeted imports at the top of each new file. No logic,
   no constant, no filter-program byte changes.
3. **Acceptance**: `sandbox_test` + `security_sandbox_escape_test` green; clippy
   `-D warnings`; every `sandbox/*.rs` ≤ 250.

## Updated Knowledge (Shadow Genome)

No new failure pattern. Confirms the child-module `impl`-split as the standard Razor
remedy for an oversized single-type file (cf. `inference_streaming.rs`,
`inference_degraded.rs`) — applied here to security-critical code with the existing
security suites as the behavior-preservation gate.

---

_Research complete. Findings advisory; the split is a mechanical, test-gated relocation of
security-sensitive code — the plan locks the exact line ranges and the audit verifies no
behavior change is proposed._
