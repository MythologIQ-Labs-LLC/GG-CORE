# GG-CORE: First Release Candidate Acceptance

**Status: convergence candidate [#139](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/139) (long-context prefill + IPC stream/non-stream controls) is the canonical release PR; its runtime tree passed the three-OS packaged real-model qualification (see the convergence section at the end). Recovery points: `9a6716d` on `main` (workflow 41914034275) and the #138 head `146bdae` (workflow 41938814283). HOLD for owner review, merge and tag.** Owner release gate: [#130](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/130). Performance qualification remains OPEN by design (not an RC blocker).

**Target:** `v0.10.0-rc.1` (the Rust package version is now `0.10.0-rc.1`; the owner has not issued a tag). A pre-release is not a claim that the public 1.0 API is stable.

## What changed in the release workflow

`.github/workflows/release.yml` now supports two paths:

- **Manual `workflow_dispatch`:** runs verification, builds GGUF-enabled CLI + C FFI for Linux, Windows, and macOS, validates packaged files and CLI version, and uploads a checksum-verified `gg-core-release-bundle` artifact. **It does not create a GitHub Release.** This is the release-package dress rehearsal.
- **Version tag `vX.Y.Z[-rc.N]`:** runs the same checks and packages, requires `Cargo.toml` to match the tag and a real matching `CHANGELOG.md` section, then publishes the assembled release with SHA256SUMS. The job refuses empty stub release notes.

All advertised standalone inference packages must actually compile the `gguf` feature. A successful `ffi`-only build is not sufficient. If any supported OS cannot build or execute a backend-enabled package, repair the platform build or explicitly narrow the supported RC matrix **before tagging**.

## Execution and evidence

1. Merge the workflow PR only after review and successful existing Rust/CodeQL CI. On `main`, open GitHub Actions → Release → Run workflow. Choose the exact release source branch/commit. Manual dispatch must leave **no** GitHub Release or tag.
2. Confirm the `verify`, all three `build` jobs, and `assemble` succeeded. Download `gg-core-release-bundle`. Inspect the three archives and run `sha256sum -c SHA256SUMS` in the extracted bundle folder.
3. On clean Linux x86_64, Windows x86_64, and the built macOS architecture, unpack the respective archive. Run the binary's `version` and `--help`. Inspect the shared library and `gg_core.h`. Verify native loader dependencies and document toolchain/runtime prerequisites.
4. Use an approved real GGUF fixture with a known SHA256 and manifest. Run `serve --model` with a nonempty `CORE_AUTH_TOKEN` and the **platform default sandbox**. Verify readiness, listing, non-streaming and streaming inference, authenticated model load/unload, failed-load cleanup, cancellation where supported, bounded resource use, and graceful shutdown. Capture exact commands, logs, model provenance/license, hashes and results without publishing model bytes or secrets.
5. **Linux sandbox gate:** prove actual token generation while default seccomp is applied, not merely successful IPC probes. Reject a `GG_CORE_SANDBOX=off` workaround as a passing sandbox test. Check for unexpected SIGSYS termination.
6. **Security gate:** privately assess filesystem-indirection/path confinement and malformed-present manifest handling; verify no unauthorized model read or silent integrity downgrade. Check default authentication boundary and file/socket permissions. Confirm Windows Job Object restrictions on a normal user account. Document that macOS currently has no enforced daemon sandbox by default.
7. **Owner gate:** enable and verify GitHub private vulnerability reporting, the channel documented in `SECURITY.md`. Maintain a dated, exact-SHA acceptance record including all platform results, residual risks, security exceptions, rollback, and explicit owner approval.
8. Only after the above is accepted, bump the source to the selected prerelease version (recommend `0.10.0-rc.1`) in `core-runtime/Cargo.toml` and `Cargo.lock`, create actual `CHANGELOG.md` notes, and correct README/SECURITY support labels. Re-run the manual dress rehearsal on **that exact source commit** before pushing the tag. Pushing the tag is an owner action.

**No substitute evidence:** A green default-feature Rust build, YAML inspection, a backend-less CLI `version` response, green CodeQL, or checksum validity alone does not prove a working inference release. A successful release workflow also does not prove every security claim.

## Performance and trust preservation (required RC evidence)

**Original design principle:** GG-CORE should deliver competitive **real inference** while providing a stronger, singular local trust boundary with blocking prompt-injection ingress checks, PII-redacting egress, authenticated lifecycle/IPC, sandboxing, resource limits and audit/health telemetry. Performance tuning must **not** weaken, bypass or quietly move any of those controls outside the trusted runtime.

### Current evidence and its limits

- `docs/BENCHMARKS.md` retains a **historical, single-host** Qwen2.5 0.5B Q4_K_M Windows/i7-7700K baseline of ~40 generated tokens/sec. This is not a current cross-platform RC result and not a fair direct comparison with other hardware/model combinations.
- `core-runtime/benches/security_overhead.rs` measures ingress/egress scan costs with synthetic data, including a PII-heavy case; `ipc_throughput.rs` measures serialization, and `inference_latency.rs` measures validation, **not model decode**.
- CI's `scripts/perf_gate.py` allows a missing baseline to skip and pass, normalizes for runner shift, and presently flags only >2.0x relative slowdowns above its noise floor. This is a **gross internal-regression smoke**, not proof of performance parity or P95 response-time quality.
- Existing `core_security_scan_latency_us` and `core_sanitize_latency_us` metrics can attribute protection cost. The local IPC metrics/status/health surfaces should stay **authenticated or explicitly minimal**, and must not become a new public HTTP management plane to satisfy Kubernetes monitoring.

### Mandatory benchmark design

1. **Same workload, provenance and hardware.** Record GG-CORE source SHA, comparator releases/commits, OS, CPU/GPU/driver, thread affinity, memory, thermal profile, runtime build flags, context/KV size, batch size, quantization, exact model artifact hash/license, tokenizer, prompt fixture hash and generator settings. Compare identical GGUF bytes where supported. If a competitor uses a different format or quantization, clearly mark it *not directly comparable*.
2. **Two operating paths, not one misleading number:** (a) GG-CORE *embedded or raw backend* versus direct `llama.cpp`/mistral.rs local engine paths, and (b) GG-CORE authenticated local IPC daemon versus the other runtimes' **equivalent local serving paths**. If comparing against HTTP, measure and disclose transport/serialization overhead separately. GG-CORE's GGUF backend uses llama.cpp through `llama-cpp-2`; do not claim a different kernel or superior decode on that basis alone.
3. **End-to-end measures:** cold model load, warm requests, prefill tokens/sec, time-to-first-token (TTFT), decode tokens/sec, inter-token latency (ITL), completed-request p50/p95/p99, memory peak, CPU/GPU utilization, cancellation, error rates, queue wait and throughput at concurrency 1 / 4 / 8 (or documented feasible levels). Include short/medium/long prompt lengths, short/long output lengths and both streaming and non-streaming.
4. **Security cost attribution:** measure default-enabled secured GG-CORE on clean, injection-attempt and PII-heavy cases; report ingress scan, output redaction and streaming holdback separately as absolute microseconds and percent of total. Compare to a **separate, isolated diagnostic** raw-compute baseline, never a production setting or unprotected public release. Require parity of security decisions and zero raw PII leakage under chunk-boundary and split-token tests.
5. **Repeatability:** predeclare warmups, repetitions (>=10 measured runs per case where feasible), randomize runtime order, pin host/caches where possible, report median plus distributions, and preserve command scripts and machine-readable results. No invented multipliers or unreplicated advantage claims.
6. **Performance regression gate:** require a same-host, same-model **pre-hardening/main candidate baseline** for real inference, or record NO-GO for *performance-qualified RC*. Existing synthetic Criterion tests remain valuable CI smoke. Do not treat a missing CI cache baseline as evidence. Proposed investigation thresholds (not yet approved promises): >10% drop in warm decode tokens/sec, >15% increase in p95 TTFT/ITL, or >10% increase in secured-path overhead relative to the pinned baseline triggers investigation and explicit disposition, not automatic security-control removal. Establish final tolerance from measured variance and owner sign-off before publishing external claims.
7. **Sandbox default:** the accepted result must be recorded with the **actual shipping sandbox and default security protections active**. A run with `GG_CORE_SANDBOX=off`, `GG_CORE_SECURITY_INGRESS=off` or `GG_CORE_SECURITY_EGRESS=off` cannot count as secured release acceptance. Nonproduction differential runs must be labeled and isolated.

**Comparators to consider:** [mistral.rs](https://github.com/EricLBuehler/mistral.rs) and [llama.cpp](https://github.com/ggml-org/llama.cpp) are priorities; others only when they share a genuinely comparable workload and release use case. Benchmark competitor code from pinned tags, not selective marketing screenshots.

**Decision:** We can issue a narrowly scoped *functional/security RC* after its separate P0 gates, with performance qualification explicitly OPEN and no unverified speed advantage. We cannot claim a **performance-qualified RC** or promote speed comparisons until the above measured evidence is accepted.

## Sign-off record (fill from actual runs)

| Gate | Receipt | Result |
| --- | --- | --- |
| Exact candidate source SHA and version | `0.10.0-rc.1` (PR #135); the candidate commit is the merge of #135 on `main`, recorded with the final preflight run in #130. Three-platform qualification receipts below were produced on the #134 head (`4e5f28b`, code identical to the candidate minus the version/docs/provenance commit) | RECORDED IN #130 |
| PR Rust / CodeQL evidence | #128, #129, #131, #132 (`44944fb`), #133 (`c5fddee`) merged with every Rust leg + CodeQL green; #134/#135 green at merge time | PASS |
| Manual Release workflow run URL, all platforms | [run 41908593547](https://github.com/MythologIQ-Labs-LLC/GG-CORE/actions/runs/41908593547): verify (fmt/clippy/default + GGUF tests/cargo-deny), build ×3 (`--locked --release --features gguf,ffi`; packaged `version`/`--help`/archive contents checked), assemble (`sha256sum -c`), qualify ×3, publish skipped. Earlier dress rehearsals: 41885727341 (main@8b977a3), 41901283318, 41903987680 | PASS |
| Linux packaged GGUF and default-seccomp real inference | ubuntu-24.04 runner, archive `gg-core-v0.9.0-linux-x86_64.tar.gz` from the same run: `Sandbox: SECCOMP`, kernel `Seccomp: 2`, non-streaming ("Paris") + streaming, unload/reload/invalid loads, auth, SIGINT exit 0 — **26/26**. Also in-container (Linux 6.18 x86_64): 31/31 incl. mid-stream client interrupt | PASS |
| Windows packaged GGUF, inference, Job Object limits | windows-2025 runner, `gg-core-cli.exe` + `gg_core.dll` from the same run: `Sandbox: ENFORCE` (Job Object: memory cap, single active process, kill-on-close), real non-streaming + streaming inference over authenticated named-pipe IPC, lifecycle/invalid loads/auth — **24/24, 1 SKIP** (graceful console-signal shutdown not drivable from git-bash; process terminated). Job Object is a resource/containment limit, not a syscall/network/principal boundary (SECURITY.md) | PASS (scoped) |
| macOS packaged GGUF and documented sandbox limits | macos-26-arm64 runner, `libgg_core.dylib` archive: `Sandbox: OFF (no implementation for this platform)` notice, real inference + lifecycle + auth, SIGINT exit 0 — **25/25** | PASS (no daemon sandbox — documented) |
| Authentication, path confinement and manifest integrity | #132: mandatory token (dev opt-in logged), socket 0600, pipe-name claim, CLI handshake; `tests/ipc_unix_socket_test.rs`. #133: symlink-aware containment (file/dir/absolute/dangling/inside/root/post-validation swap), `O_NOFOLLOW` opens, present-but-malformed manifest fails closed, post-load file identity (dev+inode / NTFS file index). Acceptance runs: wrong/empty token, traversal, corrupt and missing loads refused on all three OSes | PASS |
| ZIP advisory ignores and dependency exception review | Documented in `core-runtime/deny.toml` | REVIEWED, revisit upstream |
| GitHub private vulnerability reporting | Owner setting unverified | OPEN |
| Release artifact checksums and extracted executable tests | `assemble` job: 3 archives, `sha256sum -c SHA256SUMS`; `qualify` jobs install each archive and execute the shipped binary. Artifact download from the agent session is blocked by its egress policy — owner re-verification on a host: `gh run download 41908593547 -n gg-core-release-bundle && sha256sum -c SHA256SUMS` | PASS (CI) / OWNER RE-CHECK |
| Real-model end-to-end performance and same-host baseline | Historical ~40 tok/s record only; current measured RC baseline absent | OPEN |
| Security overhead including PII/injection and streaming | Synthetic Criterion coverage exists; end-to-end differential pending | OPEN |
| Comparable mistral.rs / llama.cpp benchmarks | No current pinned same-hardware comparison | OPEN |
| Owner GO / tag / GitHub prerelease verification | Not approved | HOLD |

## Deliberate RC limitations

The following are legitimate later work, not grounds for deleting long-term product expectations: independent security audit, stable embedded public API contract [#112](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112), pinned downstream distribution [#113](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/113), integration conformance [#114](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/114), GPU and speculative performance qualification, and model encryption integrated into the loader. They must be described accurately and never implied to be RC-verified.

**Decision rule:** unresolved P0 or missing evidence = HOLD. If a P0 requires a code change, repair it on a focused PR and repeat the applicable acceptance tests. No premature `v1.0.0`, and no release merely because the PR queue is empty.

## Post-qualification Qortara Harden acceptance (#138)

The original packaged real-inference receipts remain valid for `9a6716d` and for the behaviors actually exercised. A source-level independent `qortara-harden` review identified unqualified control-flow behavior not covered by those receipts. Before owner tagging, require the reviewed repair commit to prove:

- Authenticated stream cancellation on the originating connection **while generation is still active**, with cancel acknowledgment, an error terminal, and no continued unbounded model work.
- Request-scoped streaming timeout through queue wait and active GGUF generation, with cooperative cancellation and a final error; no success-shaped partial response.
- Unauthenticated warmup refusal with 401; authenticated warmup still works, and harmless liveness/observability probes remain available.
- Cancellation/disconnect cleanup of native generation and request/resource tracking; valid stream output and PII/injection controls still pass.
- Rust fmt, all relevant feature Clippy/tests, CodeQL, and a new exact-candidate three-OS packaged real-GGUF qualification run. Update #130 with reviewed SHA and receipts.

The fix is an RC correctness/security gate, not a mandate to add GPU or automatic model switching. The previously qualified `9a6716d` is the untouched recovery baseline; a PR being open or CI being green for source-only tests is **not** equivalent to a qualified release artifact.

## Convergence candidate acceptance (#139)

#139 combines the #138 controls with the bounded-prefill repair (long prompts no longer abort the daemon; `GG_CORE_N_CTX` is applied to the loader), then adds non-streaming `timeout_ms`, cancellation during prompt prefill (checkpoints between ≤512-token decode chunks), disconnect cleanup, and a CLI deadline instead of a fixed 5 s read timeout. The `qualify` job now runs the long-prompt, Alden-class (>2,048-token) and over-context checks plus nine IPC control checks through the packaged daemon with `GG_CORE_N_CTX=4096`.

| Runtime tree | Release run (`workflow_dispatch`, `qualify=true`, no tag) | Linux (seccomp) | Windows (Job Object, real named pipe) | macOS (no sandbox) |
| --- | --- | --- | --- | --- |
| `081de425` | 41968658661 | 35/35 | 33/33 + 1 SKIP | 34/34 |
| `37a4a71d` (same runtime; removes a tracked `.pyc`, adds ignore rules) | 41973023488 | 35/35 | 33/33 + 1 SKIP | 34/34 |

Receipts common to every platform: non-streaming `timeout_ms:350` on a ~2,500-token prompt → `deadline_exceeded` in ~350–360 ms with the next request served in 2.6–3.4 s; `cancel_request` during prefill → `cancelled` in ≤16 ms; disconnect during prefill releases the worker; cancelling one connection's request leaves a concurrent request's greedy output byte-identical; cancel → immediate unload → reload → inference. The one SKIP is the Windows graceful console-signal shutdown, not drivable from Git Bash (process terminated instead).

Gates that remain: Rust CI matrix + CodeQL on the exact head (every earlier commit on the branch skipped CI), owner review and merge of #139, a repeat of the exact-commit Release qualification on the merge commit if its tree differs, and the owner-only items (private vulnerability reporting, trusted-host bundle checksum re-verification, tag). #138 stays open as a recovery point until #139 is accepted and must not be merged independently.
