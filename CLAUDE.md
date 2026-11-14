# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Identity

**GG-CORE** (Greatest Good - Contained Offline Restricted Execution) is an **independent, upstream, standalone local inference runtime**, not an optional plugin, a UI or an external-provider router. It performs inference computation only and has no *intended* business, tool or data authority. The daemon's OS confinement varies by platform; do not claim universal isolation. See `SECURITY.md` and the producer contract.

**Dependency direction:** GG-CORE defines and qualifies its own runtime, authenticated IPC and Rust/C/Python contracts. **CoreForge is a downstream consumer** that may adopt a qualified GG-CORE version; GG-CORE must never depend on CoreForge code, UI, release timing, configuration, personas or integration tests to be a usable standalone inference product. CoreForge may host its own external-provider routing (Synapse); that is not GG-CORE functionality. A consumer does not confer GG-CORE product or release authority.

**Philosophy**: Built on triage principles ("Greatest Good for the Greatest Number"). Resource-aware, multi-tenant AI that prioritizes system stability over individual request ego.

## QoreLogic A.E.G.I.S. Lifecycle

This project uses the A.E.G.I.S. governance framework with Merkle-chained decision tracking.

**Current Status**: Check `docs/META_LEDGER.md` for lifecycle stage and next required action.

| Phase | Command | Purpose |
|-------|---------|---------|
| ALIGN/ENCODE | `/ql-bootstrap` | Initialize project DNA (CONCEPT.md, ARCHITECTURE_PLAN.md, META_LEDGER.md) |
| GATE | `/ql-audit` | Security review for L2/L3 risk grades (MANDATORY before implementation) |
| IMPLEMENT | `/ql-implement` | Execute the blueprint |
| VERIFY | `/ql-verify` | Validate implementation matches contract |

**Risk Grades**: L1 (UI only) → L2 (logic changes) → L3 (security/auth). L2/L3 require `/ql-audit` before implementation.

## Architecture Constraints

### Design Principles (C.O.R.E.)
- **Contained**: Qualified Linux seccomp daemon, Windows Job Object memory/process containment **without** network/filesystem/principal isolation, and **no macOS daemon sandbox**. In-process embedding inherits the host's privileges.
- **Offline**: No HTTP, REST, WebSocket, cloud-provider or download inference path in GG-CORE. The complete no-network rule requires host OS enforcement on Windows/macOS.
- **Restricted**: IPC-only communication with authenticated callers
- **Execution**: Pure compute, no business logic or decision authority

### Security Boundaries
| Boundary | Rule |
|----------|------|
| Process | Managed daemon separated from host; Linux seccomp, Windows Job Object resource/process limits (not AppContainer), macOS no sandbox. In-process Rust/FFI has no separate process boundary. |
| Filesystem | Model loader enforces physical `models/`/`tokenizers/` path confinement, no-follow opens and manifest integrity when present. This is **not** an OS-wide deny-all filesystem sandbox. |
| Network | GG-CORE exposes only local IPC; a platform-wide network deny is **not** enforced by Windows Job Objects or default macOS deployment. |
| IPC | Named pipes/Unix sockets only. No HTTP/REST/WebSocket/localhost ports. |

### Forbidden Modules
If these exist, scope creep has occurred—ABORT:
- `auth/`, `vault/`, `synapse/`, `plugins/`, `network/`

### Forbidden Dependencies
- `reqwest`, `hyper`, any WebSocket library, filesystem traversal libraries

**Release authority:** The published `v0.10.0-rc.1` prerelease is immutable. The standalone `v1.0.0` scope and release gate are tracked in [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) and [#161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161). No release sign/tag/publish or protected-main merge without separate owner authorization. Source/head SHA and installed real-model evidence outrank old architecture prose; a qualified in-process switch or source-only GPU path is not a published support claim.

## Code Quality Rules (Section 4 Razor)

All code must satisfy:
- Functions ≤ 40 lines
- Files ≤ 250 lines
- Nesting ≤ 3 levels

Violations block implementation.

## Current Structure (Rust)

```
core-runtime/
├── src/
│   ├── main.rs             # Entry point
│   ├── lib.rs              # Public API
│   ├── ipc/                # IPC handling, auth, protocol
│   ├── scheduler/          # Queue, priority, batching
│   ├── engine/             # Inference, tokenizer, streaming
│   │   └── moe/            # Mixture of Experts (router, executor, combiner)
│   ├── models/             # Loader, registry, lifecycle, backend dispatch (live switching unmerged)
│   ├── memory/             # Pool, GPU, cache
│   ├── telemetry/          # Prometheus metrics, OpenTelemetry spans
│   ├── ab_testing/         # A/B testing (traffic splitting, metrics)
│   │   ├── traffic/        # Traffic allocation
│   │   └── metrics/        # Per-variant statistics
│   ├── ffi/                # C FFI bindings
│   ├── python/             # PyO3 Python bindings
│   └── cli/                # Health probe CLI
├── include/                # Generated C headers (cbindgen)
├── python/                 # Python package structure
└── Cargo.toml
```

Recommended crates: `candle` or `llama-cpp-rs`, `serde`, `tokio`, `interprocess`

## Key Documents

- `docs/CONCEPT.md` - The "Why" and design philosophy
- `docs/ARCHITECTURE_PLAN.md` - File tree contract, interface specs, risk grade
- `docs/META_LEDGER.md` - Merkle-chained decision log with hash integrity
- `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` - Full technical specification
- `docs/architecture/STANDALONE_DEPENDENCY_DIRECTION.md` - Binding upstream runtime / downstream consumer boundary
