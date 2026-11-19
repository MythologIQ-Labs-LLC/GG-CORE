# GG-CORE Embedded API and IPC V1 Compatibility Baseline

**Scope:** issue [#112](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112), v1.0.0 development. This is an **external-consumer source and wire compatibility baseline**, not a frozen v1.0.0 API, stable C/Python ABI, released SDK, or approval to repin CoreForge.

GG-CORE is an independent upstream inference runtime. The canonical architecture entry point is [ARCHITECTURE_INDEX.md](../../ARCHITECTURE_INDEX.md). Implementation and tests outrank this document; feature inclusion is owned by [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) and packaged/installed release acceptance by [#161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161).

## Separate promises by interface

| Interface | Existing source-level surface | What remains unproven |
| --- | --- | --- |
| Embedded Rust | Public Runtime/RuntimeConfig and model-load/lifecycle methods; external integration tests bind selected signatures | Full v1 support list, stabilized struct-extensibility policy, backward upgrade matrix and Rust version guarantees |
| Local IPC | Tagged JSON message V1, handshake, GGUF generation, authenticated model lifecycle, typed ONNX task frames and additive capability frames | Complete producer/consumer version matrix, real packaged typed ONNX serving, strict negative and upgrade tests |
| Capability reporting | Schema version 1 and output-only compiled/detected/loaded capabilities via authenticated IPC | Verified per-model installed support; the qualified set remains empty absent trusted evidence |
| C FFI | Optional ffi feature and repr(C) types | Stable ABI symbol set, binary/header pairing, allocator and object-lifetime contract, installed cross-version proof |
| Python | Optional PyO3 python feature | Published wheels and CPython/platform ABI support matrix |
| Distribution | Current Cargo crate version 0.10.0-rc.1 and immutable published release candidate | Exact-version consumable artifact and independent rollback proof; tracked by [#113](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/113) |

**Version distinctions:** IPC V1, backend capability schema v1 and crate/release version are separate identifiers. Do not infer support or compatibility from matching numbers. The default inference boundary remains offline-first authenticated IPC without an implicit HTTP/network listener.

## Executable evidence inventory

- The preexisting external-crate test in core-runtime/tests/public_api_compat_test.rs binds historical model loading and unload signatures. It does not freeze all public Rust items.
- The protocol compatibility module documents the owner-approved non-exhaustive IpcMessage enum; external Rust matches must retain a wildcard arm. Adding an enum variant under this rule does not by itself guarantee compatibility for arbitrary old wire decoders.
- **New:** core-runtime/tests/public_contract_v1_test.rs externally binds embedded constructor/dispatch signatures and checks old V1 handshake optional defaults, additive capability frame tags, existing typed embedding request shape and nonqualified capability metadata.
- The previously merged #49 server-capability tests exercise the actual authenticated connection path, including unauthenticated refusal and malicious client-sent capability data rejection. These are not replaced by serialization fixtures.

New tests are **proposed until CI passes on the exact PR head**. Source compilation, passing CI, real-model serving, packaged acceptance and release approval are separate milestones.

## Compatibility rules pending final owner approval

1. Explicitly decide which Rust functions, public structs, feature combinations and target toolchains belong to the supported v1 API. A public Rust field is not automatically a long-term stability promise.
2. Do not silently remove or change an approved signature, JSON field meaning, error code or historic V1 frame. Preserve optional defaults for older messages.
3. New message types remain opt-in or negotiated. Never send unsolicited new frames to an old client that cannot decode them.
4. Keep Rust source compatibility, IPC wire compatibility, C ABI, Python ABI and signed binary identity separate; passing one is not proof of the others.
5. Never accept user-provided capability claims as qualification authority. Model and accelerator support require exact installed evidence.
6. Apply the final owner inclusion matrix before v1 API freeze; all promised surfaces need explicit negative, cross-version and rollback tests.

## Pre-v1 audit timestamp source-API migration (#213 / PR #215)

A direct `chrono` dependency triggered unmaintained-crate advisory `RUSTSEC-2026-0543` (maintenance, not an established exploit). PR #215 replaces it with `time` and a resolved `Cargo.lock`; the enforced supply-chain test now uses `--locked`.

**Deliberate breaking Rust source change:** the public `security::AuditEvent.timestamp` field and `security::audit::AuditLogger::get_events_by_time` arguments change from `chrono::DateTime<Utc>` to `time::OffsetDateTime`. An external Rust caller compiled against the old types must update source and use `time`. This is acceptable only under the **unfrozen v1 Rust API** review, never a claim of backward compatibility with already compiled binaries or v0.10.0-rc.1 source consumers. No C ABI/IPC field contract is intentionally changed by this migration.

**Preserved historical audit behavior:** RFC3339 timestamp strings with UTC `Z` and fractional precision deserialize, and offsets such as `-05:00` normalize to UTC on read. Serialized audit timestamps are always canonical UTC even when an external caller assigns a non-UTC `OffsetDateTime`. Human audit logs label UTC **only after conversion**. External-crate tests in `core-runtime/tests/audit_timestamp_compat_test.rs` cover offset normalization, fractional precision, malformed input rejection, time-range comparison and exported JSON.

This is **source/wire test coverage**, not an installed compatibility claim. Before the v1 stable API freeze, #112 must enumerate the exact retained Rust signatures, and consumer migration notes must accompany a versioned SDK. The existing immutable RC1 tag/binaries are unchanged.

## Pre-v1 status latency quantile correction (CLI/JSON)

Before this change, the `gg-core-cli status` implementation reported `p50_latency_ms` as the **minimum**, `p95_latency_ms` as `0.95 * maximum`, and `p99_latency_ms` as `0.99 * maximum`. These are **not sample percentiles**. The existing authenticated metrics snapshot exposes only `HistogramSummary { count, sum, min, max }`, insufficient to reconstruct any percentile from a multi-observation distribution.

**Corrected source semantics:** `RequestStats.{p50,p95,p99}_latency_ms` are now `Option<f64>` in the Rust status API; serialized `status --json` fields are explicitly `null` when an observed quantile is unavailable. Human `status` shows `N/A` rather than fabricating milliseconds. The average remains the measured `sum/count` for a nonempty histogram. No new runtime telemetry collector, network transport or quantile computation has been invented.

**Related model-status truthfulness corrections:** `ModelInfo.avg_latency_ms` is already the mean when produced by `ipc::handler` (`total_latency_ms / request_count`). The old CLI `build_models_list` divided that mean by request count **again**, underreporting latency as traffic increased. It now preserves the IPC value unchanged. An unrecognized model `state` string previously defaulted to `Ready` and now maps conservatively to `Error`; unsupported producer states must never certify readiness. The Rust `ModelStatus` field types and known V1 model-state names remain unchanged. Source regressions cover both corrections.

**Compatibility boundary:** this deliberately changes a **pre-v1 public Rust source type** from `f64` to `Option<f64>` and a CLI JSON value from a number to `null`. Rust consumers must handle `None`; JSON clients must accept nullable percentile fields. This is not wire compatibility with code that required a number. The V1 embedded/CLI status contract remains **unfrozen**, and this behavior requires explicit inclusion in #112 before freeze. This does not change the authenticated inference IPC framing or GGUF/ONNX execution paths.

To implement real percentiles later, a trusted recorder must retain samples or a properly validated quantile distribution, attribute metrics to correct model/task and window, and independently verify end-to-end inference timing. Fake P95/P99 must never be used to qualify model selection, performance baselines or release support. The regression test constructs an adversarial `min=1, max=500, count=10, sum=1000` summary and verifies JSON `null` and `N/A`, not manufactured P50/P95/P99. This is source behavior, **not** a hardware benchmark or installed release receipt.

### Advanced speculative status counters versus unavailable measurements

With the `advanced` feature, the authenticated metrics snapshot carries speculative draft/accept/reject counters but **does not carry** the measured per-step draft latency, verification latency or net speedup. The CLI aggregation currently populates those three numeric `SpeculativeSessionStats` fields with placeholder `0.0`; those values are **not observations**. Human-readable `gg-core-cli status` now displays `N/A (IPC)` for them instead of presenting fictitious `0.00x` speedup or `0.0 us` timing. It continues to display the actual transported counts and derived acceptance rate.

**Pre-v1 JSON/source limitation:** `status --json` still emits numeric placeholder fields inside `speculative_stats` for backward source-shape continuity; clients **must not** infer measured throughput, speedup or latency from those zeros. Converting the fields to nullable, removing them or adding trust/availability flags requires an explicit compatibility decision under #112 and external consumer tests. This is not evidence that speculative decoding is performing or production-qualified.

### Incomplete status IPC is degraded, never silently healthy

`gg-core-cli status` obtains health, metrics and model-list responses over separate IPC requests. A successful health request does **not** mean the later metrics and registry requests succeeded. The CLI now preserves the daemon's **full health report** state instead of treating the response's `ok` readiness boolean as the complete state: a saturated queue or require-model violation is **`degraded`**, not automatically `unhealthy`; shutdown and non-acceptance remain `unhealthy`. A missing full health report or missing metrics/model response cannot certify healthy and emits a `status_ipc` warning for the unavailable section. With both requests successful, an intentionally empty model list or zero-valued metrics remains healthy; an actual failed health response remains **`unhealthy`** even if other sections are available. This does not assert a daemon or model failure merely because a read failed; it prevents a partial diagnostic from being misinterpreted as complete positive evidence.

CLI `status` remains an informational command, so callers requiring operational readiness must inspect the explicit JSON `health` value or use the dedicated `ready`/health path. Existing numeric status fields can still be zero when telemetry is unavailable, but their status is explicitly **degraded**, not a verified zero measurement. This is a pre-v1 CLI diagnostic correction; it does not change the authenticated inference IPC protocol or grant any new authority.

## Remaining dependencies and exit criteria

- [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160): owner-approved supported API, language, platform and model/task matrix.
- [#114](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/114): reusable external conformance harness and actual producer binaries.
- [#113](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/113): exact-version distribution and rollback.
- [#171](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/171) and [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137): real typed ONNX IPC serving and secure A-to-B-to-A model lifecycle.

Do not close #112 merely because this initial baseline merges. Close only after the supported public surface is approved, exact-head compile/wire/ABI tests satisfy that surface and independent review verifies compatibility. No v1.0.0 release or CoreForge pin change is authorized here.
