# Backend capability contract and authenticated snapshot: evidence (#49 / #50)

**Branch:** `feat/v1-backend-capability-contract-20261113-no-ci`, based on `main` `1032d1ba`.

| Commit | Content |
|---|---|
| `17526ba8` | Authored schema and selection |
| `044cf7c` | Review fixes: `#[non_exhaustive]` and Serialize-only claims; rustfmt |
| `fe29287` | Clippy lifetime fix |
| `b9dbe17` | Authenticated snapshot and explained eligibility |
| `120118d` | Review fixes: detected-backend test and doc limits |

**Qualified commit: `120118d`.**

**Host and scope:**
- **Host:** offline Windows Server 2025 build VM, rustc 1.99.1. No GitHub Actions minutes were used.
- **Scope:** source tests only. The snapshot was exercised through the real IPC connection loop with a stub model. No installed-product or packaged evidence exists, and **zero production support cells are qualified, by design.**

## Gate history (batches)

| Batch | Commit | fmt | Clippy `-D warnings` (5 sets) | default / advanced / full |
|---|---|---|---|---|
| local check | `17526ba8` as authored | **FAIL** (23 diffs) | not run | not run |
| `v1b` | `044cf7c` | pass | **FAIL** (`needless_lifetimes`) | 1500 / 1635 (3 ign) / 1601 (1 ign), 0 failed |
| `v1d` | `fe29287` | pass | pass | 1500 / 1635 (3) / 1601 (1), 0 failed |
| `v1f` | `b9dbe17` | pass | pass | 1512 / 1647 (3) / 1613 (1), 0 failed |
| **`v1g`** | **`120118d`** | **pass** | **pass** | **1513 / 1648 (3) / 1614 (1), 0 failed** |

The `v1a` Branch B leg never ran: a shared-runner `PATH` defect made its results file report `exit 0` falsely. It is discarded.

## New tests (all run, all pass at `120118d`)
- **Snapshot** (5 tests):
  - a backend that cannot be identified gives `declared_only_paths` only, and NER is omitted;
  - hardware and compiled paths never become `qualified` or `loaded`, and there is no CUDA or Metal path;
  - operator constraints come from the configuration (`auth_required`, and the budget or `None`);
  - an engine model without a lifecycle handle is omitted;
  - a detected backend serves only its compiled declared paths, with no ONNX generation.
- **IPC through `handle_connection`** (4 tests):
  - unauthenticated: 401, then the connection closes;
  - authenticated: the loaded stub is reported truthfully (`backend: null`, declared-only);
  - a client-sent `capabilities_response`: 400, the connection stays open, nothing is ingested;
  - `require_auth = false`: the frame is open and reports `auth_required: false`.
- **Eligibility** (4 tests): every rejection reason; streaming and pinning without substitution; ambiguity selects nothing, with deterministic results; exactly one qualified entry is selected.
- **Compatibility:** the `protocol_compat` doctest and the in-crate guard both cover 30 variants.

## Reviews
- **Authored schema review** (this session): two issues found and fixed in `044cf7c`.
  - The enums and structs were not `#[non_exhaustive]`, a #112 source-compatibility risk.
  - `RuntimeBackendCapabilities` and `ServingPath` derived `Deserialize`, which risked trusting client-supplied claims. They are now Serialize-only.
- **Snapshot plan L2 audit:** VETO, then PASS with conditions P1–P5, all met (see `V1_CAPABILITY_SNAPSHOT_PLAN.md`).
- **Implementation review:** APPROVE WITH CONDITIONS, no correctness or security defects. All findings were fixed in `120118d`.

## Not established
- An evidence registry populating `qualified`.
- Accelerator reporting.
- Model selection, memory-budget eligibility and host authority (deferred in writing to #50 / #153).
- A CLI command.
- A real ONNX/GGUF model in the snapshot test. The downcast is unit-tested by type, and the real-model path is not exercised.
- Linux and macOS.
