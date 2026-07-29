# Plan: B-16 — `sandbox/unix.rs` Section 4 Razor Refactor

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - **Verification is CI-based.** `sandbox/unix.rs` is `#[cfg(unix)]`-gated
    (`sandbox/mod.rs:5`); the dev host is `x86_64-pc-windows-msvc`, so `unix.rs` and the
    sandbox/seccomp security suites do **not** compile locally. Local verification proves
    only that the crate still builds on Windows (cfg wiring intact); correctness of the
    Unix split is verified by the CI matrix (`.github/workflows/rust.yml`, Linux + macOS).
    The `/qor-substantiate` seal is held until CI is green (operator-authorized push).
- non_goals:
  - Any change to sandbox behavior — seccomp filter bytes, cgroup writes, syscall
    whitelist, or the `Sandbox` trait impl. This is a **pure relocation**.
- exclusions:
  - No new tests (the existing `sandbox_test` + `security_sandbox_escape_test` suites are
    the behavior-preservation gate); no logic edits.

## Open Questions

None. Research (#141) specified a mechanical concern-split; operator authorized
refactor-then-push-then-seal-after-CI-green.

## Design Rationale (Simple Made Easy)

`unix.rs` (535 lines) holds one `UnixSandbox` type with three separable concerns. A
type's `impl` may span sibling files via child modules that keep private-field access
(the in-repo `inference_streaming.rs` / `inference_degraded.rs` pattern). Each concern
moves **verbatim** to a child-module `impl UnixSandbox` block; the only edits are module
declarations, `pub(super)` on the two methods `apply` calls across the boundary, and
per-file imports. No logic, constant, struct, or filter byte changes.

## Phase 1: Extract seccomp + cgroup + tests into sibling files

### Affected Files

- `core-runtime/src/sandbox/unix_seccomp.rs` (NEW, ~230 lines) — moved **verbatim** from
  `unix.rs`: the `#[cfg(target_os="linux")]` consts `SECCOMP_MODE_FILTER`/`SECCOMP_RET_ALLOW`/
  `SECCOMP_RET_KILL_PROCESS` (lines 32–41), `bpf`/`bpf_size`/`bpf_mode`/`bpf_src`/`bpf_jmp`
  submodules (43–99), `AUDIT_ARCH_X86_64`/`AUDIT_ARCH_AARCH64` (102–108),
  `SeccompData`/`SockFilter`/`SockFprog` structs (110–139), and an `impl UnixSandbox` block
  with `gpu_syscalls_x86_64` (218–230) + `apply_seccomp_filter` (both the linux form 232–388
  and the non-linux stub 390–393). `apply_seccomp_filter` becomes `pub(super)`. Header:
  `use super::UnixSandbox;`.
- `core-runtime/src/sandbox/unix_cgroup.rs` (NEW, ~70 lines) — moved verbatim: consts
  `CGROUP_V2_BASE`/`SANDBOX_CGROUP_NAME` (25–29), and an `impl UnixSandbox` block with
  `cgroups_v2_available` (158–163) + `apply_cgroup_limits` (165–216). Both become
  `pub(super)`. Header: `use super::UnixSandbox;` `use std::fs::{self, OpenOptions};`
  `use std::io::Write;` `use std::path::Path;`.
- `core-runtime/src/sandbox/unix_tests.rs` (NEW, ~45 lines) — moved verbatim: the bodies of
  `#[cfg(test)] mod tests` (495–534), i.e. the three tests. Header: `use super::*;`.
- `core-runtime/src/sandbox/unix.rs` — **retains** the module doc, the struct `UnixSandbox`
  (141–146), `impl UnixSandbox { new }` (148–156), and `impl Sandbox for UnixSandbox`
  (396–489). Trim imports to those still used here (`use super::{Sandbox, SandboxConfig,
  SandboxResult, SandboxUsage};`, `use crate::telemetry::{log_security_event, SecurityEvent};`,
  `use std::fs;` — drop `OpenOptions`, `std::io::Write`, `std::path::Path`, now used only in
  the child modules). Add three module declarations:
  ```rust
  #[path = "unix_cgroup.rs"]
  mod cgroup;
  #[path = "unix_seccomp.rs"]
  mod seccomp;
  #[cfg(test)]
  #[path = "unix_tests.rs"]
  mod tests;
  ```
  End size ~150 lines. `apply` still calls `self.apply_cgroup_limits()` and
  `self.apply_seccomp_filter()` (now `pub(super)` in the child modules — reachable from the
  parent). `get_usage` keeps `fs::read_to_string`.

### Changes

Verbatim relocation. Visibility contract: `pub(super)` on `apply_seccomp_filter`,
`apply_cgroup_limits`, and `cgroups_v2_available` (`pub(in unix)` — visible in `unix` and
all its descendant modules, so `apply` and the moved tests both reach them). No other edit.

### Unit Tests

- No new tests. The relocated `unix_tests.rs` (`test_cgroups_v2_detection`,
  `test_sandbox_disabled_by_config`, `test_sandbox_enabled_returns_proper_error`) plus the
  crate-level `sandbox_test` + `security_sandbox_escape_test` suites are the
  behavior-preservation gate. They run on CI's Linux + macOS legs (not locally on Windows).

## Feature Inventory Touches

Empty — justified. Pure internal file-split refactor of an existing subsystem (F-38 Sandbox
isolation); no user-touchable feature introduced or modified. Behavior is unchanged, gated
by the existing sandbox suites.

## Definition of Done

### Deliverable: `unix.rs` split to ≤250-line files, behavior unchanged

- **D1**: `sandbox/unix.rs` and every new `sandbox/unix_*.rs` are ≤250 lines; the sandbox
  behaves identically (seccomp filter + cgroup enforcement byte-identical).
- **D2**: New `unix_seccomp.rs` / `unix_cgroup.rs` / `unix_tests.rs` hold the relocated
  `impl UnixSandbox` blocks + items; `unix.rs` retains the struct, `new`, `impl Sandbox`,
  and the three `mod` declarations; `apply_seccomp_filter`/`apply_cgroup_limits`/
  `cgroups_v2_available` are `pub(super)`.
- **D3**: META_LEDGER entry (canonical markup) records the split; BACKLOG B-16 → done;
  SYSTEM_STATE sandbox tree updated.
- **D4.d**: Local verification is impossible on the Windows dev host (`unix.rs` is
  `#[cfg(unix)]`). **Follow-up phase**: CI matrix (`.github/workflows/rust.yml`, Linux +
  macOS legs) compiles `unix.rs` + runs `sandbox_test`/`security_sandbox_escape_test`; the
  seal is held until those legs are green.

## CI Commands

```bash
cargo build -p gg-core                                                  # local: crate still builds on Windows (cfg wiring intact)
cargo clippy -p gg-core --all-targets --all-features -- -D warnings     # CI (Linux/macOS): compiles unix.rs; warnings-as-errors
cargo test -p gg-core --test sandbox_test                               # CI: sandbox behavior unchanged
cargo test -p gg-core --test security_sandbox_escape_test               # CI: sandbox-escape defenses unchanged
cargo fmt --check                                                       # formatting
```
