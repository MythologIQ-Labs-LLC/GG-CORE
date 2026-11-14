# GG-CORE RC1: Release Support and Maintenance

**Scope:** Local, CPU-based GGUF inference with one resident model and authenticated IPC. GG-CORE is an inference component, not a desktop application, UI, optional inference plugin, or external-provider router. The consuming application owns model choice, its interface, installation and process supervision. This guide does **not** claim that the model-switching, GPU, performance, or stable embedded API milestones are finished.

**Source of truth:** [Release gate #130](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/130) and the [GitHub Releases page](https://github.com/MythologIQ-Labs-LLC/GG-CORE/releases). On 2026-11-05, `main@d55ba50a262469d4cee828e9c6431670635c3cc5` passed Rust CI, CodeQL, and full packaged three-platform real-model qualification; **no public `v0.10.0-rc.1` tag or release was yet published**. If documentation changes are later merged, the new source SHA must be requalified before tagging. Do not mistake a candidate badge for a published download.

## What is supported

| Surface | Current release-candidate status |
| --- | --- |
| Local daemon on Linux x86_64 | GGUF/CPU with authenticated Unix socket and default seccomp sandbox; packaged real-model acceptance passed |
| Local daemon on Windows x86_64 | GGUF/CPU with authenticated named pipe, Job Object memory/process limits; **not** a syscall, filesystem, network or principal sandbox |
| Local daemon on macOS arm64 | GGUF/CPU with authenticated Unix socket; **no enforced daemon sandbox** |
| Rust library and FFI | Implemented; the host must enforce its own process trust boundary for in-process execution |
| Python bindings | Code and CI feature leg exist; not a published wheel with installation promise |
| Performance, automatic model switching, GPU, ONNX text generation | Not qualified for this RC |

A local model must be present and authorized. GG-CORE does **not** fetch models or grant network, tool or business authority. It does not manage external-provider credentials or routing. The application using GG-CORE is responsible for those decisions.

## Before an application pins this release

1. **Use an exact published SHA or tag.** Do not float `main`. Require a qualified release tag and version/provenance `gg-core-cli version --json` that matches it.
2. **Inspect and verify archives.** From the downloaded release bundle, run `sha256sum -c SHA256SUMS` and compare file provenance to the release record. Confirm the packaged CLI includes `gguf` in `features`. Check the OS/architecture and model license separately.
3. **Protect the local boundary.** Generate a high-entropy `CORE_AUTH_TOKEN` of at least 16 bytes. Store it in the application's credential store, not a repository, log, command history or screenshot. Do not use `GG_CORE_ALLOW_INSECURE_NO_AUTH=1` in installed user software.
4. **Allocate enough memory.** The daemon's `GG_CORE_MAX_TOTAL_MEMORY` default is max(2 GiB, 50% of physical RAM); an explicit value is an exact ceiling and is never raised. Explicit values must be plain decimal byte counts of at least 1 MiB, and the per-call value may not exceed the total; an invalid or contradictory value makes `serve`, `config show` and `config validate` fail instead of running with a different limit. On Windows the total is also the Job Object memory cap (unless `GG_CORE_SANDBOX=off`); on Linux it is an OS (cgroup) cap only with `GG_CORE_SANDBOX=enforce` (default seccomp mode enforces it through GG-CORE admission only); macOS has no OS cap. These variables govern the standalone daemon and CLI; embedded Rust/FFI/Python hosts pass their own `ResourceLimitsConfig`. Larger context/model combinations may require a measured, explicitly higher limit. Native `GG_CORE_N_CTX` defaults to **2048**, even though admission limits default to **4096**. For long-prompt qualification, set `GG_CORE_N_CTX=4096`, `GG_CORE_MAX_CONTEXT=4096`, and `GG_CORE_MAX_CONTEXT_TOKENS=4096` together.
5. **Keep one resident model.** The daemon supports authenticated load and unload; automatic hardware/model-tier selection and dynamic hot swap remain future work. Do not infer those features from a working `models load`.
6. **Keep process ownership clear.** A host may launch GG-CORE as a managed child and use its local socket/pipe. Host-owned UI, install/update policy, choice of external inference and application actions are **not** GG-CORE responsibilities. Read the [producer contract](integration/COREFORGE_PRODUCER_CONTRACT.md).

## Minimal functional smoke test

Use a genuine permitted `models/example.gguf` in the daemon's configured base path. The string is a placeholder filename, not a downloadable model.

**Linux/macOS shell** (two terminals, same token and working-directory configuration):

```bash
export CORE_AUTH_TOKEN='<replace-with-a-long-random-secret>'
# The 2048-token native default is sufficient for this short smoke test.
# For longer prompts explicitly set N_CTX and admission limits together.

# From the configured base directory, with the installed CLI on PATH:
gg-core-cli version --json
gg-core-cli serve --model models/example.gguf --model-id local-model
```

From the second terminal with the same `CORE_AUTH_TOKEN` and socket path:

```bash
gg-core-cli live
gg-core-cli ready
gg-core-cli models list --json
gg-core-cli infer --model local-model --prompt 'Answer with a short greeting.' --max-tokens 32
gg-core-cli infer --model local-model --prompt 'Count to three.' --max-tokens 32 --stream
```

**Windows PowerShell:** Set `CORE_AUTH_TOKEN` and any selected context values with `$env:NAME = 'value'`; start `gg-core-cli.exe serve --model models/example.gguf --model-id local-model`. Use `gg-core-cli.exe version --json`, `ready`, and `infer --model local-model` in a second PowerShell with the same token and selected named-pipe path. The Windows termination procedure must be supervised by the consuming host; do not assume a Unix signal behavior.

**Expected:** `live` can succeed without a model, but `ready` should succeed only after a model is usable. A failed preload must abort startup, not report a serving model. A real inference must return sanitized text, not just a healthy process, probe or backend-less version banner.

## If something fails

| Symptom | Check first | Safe next action |
| --- | --- | --- |
| `serve` refuses to start | Missing/short `CORE_AUTH_TOKEN`, invalid preload path, missing model, failed integrity/manifest or sandbox setup | Fix the configuration/model, leave authentication and sandbox enabled |
| `live` works but `ready` fails | Model not yet loaded or preload failed | Check `models list --json`, exact model ID, model directory and manifest |
| `infer` reports not authenticated | Client has wrong token or opened a new connection without handshake | Use matching host-managed secret; do not disable auth |
| Model ID not found | `--model` references a model *ID*, not the GGUF filename | Use the exact `--model-id` supplied to preload or `--id` supplied to load |
| Long prompt rejected | Native `GG_CORE_N_CTX` is 2048 by default despite 4096 admission limits | Choose suitable model/context; set all three context variables before loading and measure memory |
| Unexpected deadline or delayed release | Queue wait or prefill exceeded budget; cancellation checkpoints occur between native batches | Inspect IPC error code, request deadline and host memory/CPU pressure; do not treat a cancel ACK as instant native teardown |
| Windows sandbox confusion | Job Object does not block network or arbitrary filesystem access | Run with least privilege; use OS/AppContainer policy only after separate host qualification |
| macOS sandbox warning | No daemon OS sandbox exists for the RC | Respect that limitation; use OS-level isolation if required by deployment policy |
| Package will not run | Wrong CPU/OS build, missing loader dependency, corrupted archive or source mismatch | Verify archive hash, `version --json`, architecture and platform runtime dependencies; never ignore integrity failures |
| Performance feels slow | Historical token/sec figures are not guarantees; model/context and secured-path cost vary | Capture repeatable measurements with protections enabled; follow [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136), not external speed claims |

Never include live authentication tokens, model contents, PII or complete prompts in public troubleshooting reports. Attach a redacted log, OS version, `version --json`, model filename/hash if shareable, requested context size, and the error code instead.

## Release finalization and rollback

**Before first tag:** exact candidate SHA must have green Rust, CodeQL, and the nonpublishing Release job with installed real-GGUF inference on all three platforms; independent archive checks; owner verification of private vulnerability reporting; and explicit publication authorization. A docs-only merge still changes the commit and reopens the qualification SHA gate.

**At publication:** tag the qualified merge SHA, allow the tag-triggered release workflow to build/qualify/publish, then download the published archives on trusted hosts, verify `SHA256SUMS`, run `version --json` and a real-model first-run test. Do not close #130 until that post-publication acceptance exists.

**Prefill split compatibility (N7 successor, #184):**
- **What changed:** plain prompt prefill now decodes in a deterministic, position-based chunk schedule (`engine/gguf/prefill_schedule.rs`), whether or not the request can be cancelled. Previously the split was a native 512-token batch, or time-paced in the C2 candidate.
- **Effect on output:** for prompts longer than one chunk, greedy output can differ from RC1/C1/C2. It is reproducible for the same model file, thread bucket ({1}, {2-3}, {4-7}, {8+}), prompt and sampling, under any load or concurrency.
- **Embedded vs daemon:** these can land in different buckets on one host. Embedded auto threads are `min(logical,16)`; the daemon divides that by its worker count. Their output can therefore differ.
- **Rollback:** rolling back to C2 or RC1 restores the previous split.

**Rollback:** stop the consuming host's GG-CORE child process, return to the previously approved exact binary and recorded config/model combination, validate hash/version, reload its one permitted model, and repeat readiness plus inference. Do not force-update users by moving a tag or silently following `main`. A defective prerelease should be superseded by a **new immutable version/tag**, not replaced in place.

## Maintenance policy: what merits returning to GG-CORE?

| Trigger | Action |
| --- | --- |
| Security advisory or integrity/authentication defect | Open a focused P0, test and requalify the new exact SHA; consider an out-of-band RC patch |
| Real-model crash, resource leak, or incorrect output | Reproduce against the installed package with a minimal redacted fixture, add regression coverage, requalify |
| OS/toolchain change breaks a supported platform | Isolate the platform/ABI issue and rerun affected tests plus full release gates before a new tag |
| Performance or new model/backend request | Track in [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136), [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137) or the future-backend issues; do not silently expand RC support |
| Downstream app integrates external providers, UI or tools | Handle in the consuming application's repository, not GG-CORE |

**Rust source compatibility of `IpcMessage` (#112, owner disposition 2026-11-13):**
- **RC baseline:** the enum was exhaustive in `v0.10.0-rc.1`. `main` then added two frame types (#171), and #183 adds two more.
- **Change:** the enum is now `#[non_exhaustive]`. External crates must match it with a wildcard arm (`ipc::protocol_compat` doctests enforce this), and later frame types are not source-breaking.
- **Scope:** this covers the IPC enum only. The wider embedded API contract stays open in #112.

**Deferred by design, not lost:** [#112](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112)–[#114](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/114) (stable embedded API/versioned distribution/conformance), [#115](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/115) (ONNX generation research), [#48](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/48)–[#51](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/51) (capability schema/hardware selection/BitNet), and [#70](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/70) (research). These remain open rather than being falsely marked complete. Do not expand the initial RC to satisfy them.
