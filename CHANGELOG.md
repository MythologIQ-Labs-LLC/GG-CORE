# Changelog

All notable changes to GG-CORE (Greatest Good - Contained Offline Restricted Execution) are documented in this file.

## [Unreleased]

### Changed (model release wait, #183)
- `gg-core-cli models unload <id> --wait-ms N` exits `0` only when the model's memory was released, and `4` when the wait expires with instances still held. It exits `1` when a release receipt names another model or is self-contradictory. Plain `models unload <id>` is unchanged.
- **Rust source compatibility:** `ipc::protocol_types::IpcMessage` is now `#[non_exhaustive]`. This is a one-time source break for external crates that match it exhaustively; add a wildcard arm. Later frame types are then not breaking (#112). The wire format is unchanged.

## [0.10.0-rc.1] - 2026-11-04

### Producer convergence after the initial short-prompt acceptance

- Process stream cancellation concurrently with IPC reads so an authenticated caller can cancel an active response on the same connection; cancellation and disconnect now propagate to the native generation loop.
- Bound streaming requests by a wall-clock deadline and ensure cancellation/timeouts return a failed terminal rather than false successful completion.
- Authenticate compute-producing warmup operations, retaining unauthenticated read-only health and monitoring probes.
- Keep one process-wide llama.cpp backend so `model_unload_request` → `model_load_request` succeeds while a cancelled or expired stream's native loop is still winding down to its next checkpoint (previously the reload failed with `BackendAlreadyInitialized`). The packaged acceptance now exercises cancel → immediate unload/reload → inference.
- `inference_request.parameters` fields are optional on the wire, as the IPC schema has always documented; a request omitting `temperature`/`top_p`/`top_k`/`max_tokens` now takes the documented defaults instead of failing with a serialization error.

- **Fixed: non-streaming `timeout_ms` was ignored once a request started** (a `timeout_ms:350` request ran 32–40 s and returned a success), and a non-streaming request could not be cancelled because the connection loop awaited it inline. Non-streaming requests now run beside the connection reader: `timeout_ms` is a wall-clock deadline (queue, prefill, generation) answered at expiry with `error_code:"deadline_exceeded"`; `cancel_request` and disconnect stop the request with `error_code:"cancelled"`. A stopped request never returns partial output as a success, even if a backend ignores the checkpoint. The embedded `Runtime::infer` honors `timeout_ms` the same way (`InferenceError::DeadlineExceeded`, FFI `Timeout`/`Cancelled`). `timeout_ms:0` is rejected as invalid input. Non-streaming native inference now runs on the blocking thread pool (as streaming already did), so a long prefill can no longer starve the connection task that delivers `cancel_request` or a disconnect; a backend panic becomes an explicit `execution_failed` instead of ending the worker loop. Requests cancelled or expired before execution now get an explicit error instead of a dropped response channel. New protocol `error_code` values: `cancelled`, `deadline_exceeded`.
- Cooperative cancellation checkpoints in GGUF prompt **prefill** (streaming and non-streaming): before context creation and between decode chunks (llama.cpp's physical batch, at most 512 tokens per chunk). A cancelled or expired long prompt stops within one chunk instead of finishing the whole prefill; its context, KV cache, queue slot and memory reservation are released and the next request runs. Real-model tests cover prefill cancellation, non-streaming deadline expiry, concurrent-request isolation and recovery; the packaged acceptance probe adds non-streaming prefill deadline, `cancel_request`, disconnect and isolation checks on every platform.
- **Fixed: `gg-core-cli infer` gave up on any response slower than 5 s** (a fixed client read timeout), so a long non-streaming prompt failed with `Timeout waiting for response` while the daemon was still computing it. `infer` now sends a daemon-enforced `timeout_ms` (default 300 000, `--timeout-ms` to change) and waits 10 s beyond it, so a slow request ends with the daemon's explicit `deadline_exceeded` instead of a client-side guess. Found by the first three-OS packaged run of the long-prompt acceptance checks.
- Bound GGUF prompt prefill to the actual llama.cpp batch and microbatch capacities instead of issuing one unbounded decode that could abort the daemon. Reject prompts exceeding the configured model context with an ordinary input error, preserve the process, and cover long prompts with a real-model fixture and packaged daemon acceptance checks.
- Apply `GG_CORE_N_CTX` and `GG_CORE_N_THREADS` to the model loader rather than only parsing and displaying the settings. Verify >2,048-token prompts using the configured 4,096-token context.

**Release candidate (functional/security scope, pending exact-artifact requalification).** The earlier `main@9a6716d` was qualified in #130 with packaged `gguf,ffi` binaries on Linux x86_64, Windows x86_64 and macOS, each exercised with a real Qwen2.5-0.5B-Instruct Q4_K_M model and platform-default sandbox. That historical receipt does **not** qualify the later combined long-prompt and IPC-control corrections. The converged source received separate Windows VM qualification at `0f4a1bbd`, and the non-streaming deadline/prefill-cancellation fixes were re-qualified in the same Windows VM against the real Qwen fixture; all three packaged-platform acceptance gates must pass again on the exact final release commit before tagging. Declared scope: CPU inference, GGUF backend, one actively resident model per daemon, authenticated local IPC, non-streaming and streaming generation. **Not RC-qualified (tracked, not hidden):** performance qualification against a same-host baseline, adaptive model switching with real memory reclamation, GPU acceleration, ONNX as a release surface, macOS daemon sandbox (none enforced), the stable embedded public API contract (#112), pinned downstream distribution (#113), integration conformance (#114), independent audit.

### Security
- Model-load confinement and integrity (RC acceptance gate, issue #130): path confinement is now **physical** as well as lexical — an existing entry under `models/`/`tokenizers/` must resolve (symlinks, linked directories) under the real path of an allowed root (`models/containment.rs`); escapes are refused, logged as `path_traversal_attempt`, and never echo the link target. Model files are opened with `O_NOFOLLOW` for hashing and mmap, so a link swapped in after validation is refused. A sibling `manifest.json` that **exists must parse and validate** or the load fails (previously a malformed manifest was silently treated as absent, dropping the integrity check); the hashed file's identity is re-checked after the backend loaded it by path, so a rename/symlink substitution between hash and load fails the load with nothing registered. New `tests/security_containment_test.rs` (symlinked file/dir/absolute/dangling/inside/root cases + post-validation swap), manifest fail-closed tests, identity-swap tests.
- Daemon trust boundary (RC acceptance gate, issue #130): `serve` now **refuses to start with an empty `CORE_AUTH_TOKEN`** — previously an empty token let every local client that could reach the socket authenticate. `GG_CORE_ALLOW_INSECURE_NO_AUTH=1` is the explicit, development-only opt-in (banner on every start + critical `insecure_configuration` security event). The Unix socket is now created owner-only (`0600`); the Windows named pipe claims its name with `first_pipe_instance(true)` (a pre-existing pipe — squatting/impersonation — fails startup) and rejects remote clients. New `tests/ipc_unix_socket_test.rs` drives the real server loop over a Unix socket (socket mode, unauthenticated probes, CLI auth fail-closed/pass).
- **Fixed: the shipped CLI could not run inference.** `gg-core-cli infer` (streaming and non-streaming) never performed the session handshake, so a token-protected daemon answered `Not authenticated`; the real-model acceptance run surfaced it. The CLI now handshakes on the inference connection (`cli/ipc_client_auth.rs::handshake_on`), and streaming failures the daemon reports in a chunk's `error` field (unknown model, validation, engine errors) are surfaced as CLI errors instead of a silent empty line.
- **Fixed: streaming inference requests could sit unserviced indefinitely.** `RequestQueue::wait_and_dequeue` re-awaited the shared `Notify` whenever the *main* queue was empty, swallowing the wake-up that `enqueue_streaming` sends — a streaming request was only executed once unrelated non-streaming traffic woke the worker (the acceptance run's first streaming call took 26 minutes, until a later probe arrived). It now returns when streaming work is pending so the worker loop services that queue. Two guards ride along: a stream whose producer drops without a terminal now yields a final error chunk to the client instead of an open-ended wait (`handler::relay_stream`), and the non-gguf worker path terminates the stream instead of dropping it. Regression tests at queue level (`streaming_enqueue_unparks_wait_and_dequeue`) and worker level (`streaming_request_is_serviced_by_an_idle_worker`, enqueues *after* the worker is idle — the existing tests enqueued first and so never hit the bug).
- The CLI now treats an `InferenceResponse` carrying `error` (unknown model, admission rejection, validation) as a failure (non-zero exit, message on stderr) instead of printing an empty line and exiting 0.
- **Fixed: the Windows daemon would have terminated itself ~30 CPU-seconds into inference.** The daemon's Job Object inherited the library default `max_cpu_time_ms = 30_000`, which Windows enforces as `JOB_OBJECT_LIMIT_JOB_TIME` — a cumulative user-CPU budget that kills every process in the job when exhausted. `sandbox_boot::daemon_sandbox_config` now sets it to 0 (memory remains the gate). The Job Object additionally limits the job to one active process (no child process creation — the analogue of the seccomp `execve`/`fork` denial), kills on job-handle close and dies on unhandled exceptions. SECURITY.md now states what the Windows sandbox is (resource/containment limits, not a syscall or network filter) and that macOS has no enforced daemon sandbox.
- Real-model sandbox acceptance on Linux x86_64: Qwen2.5-0.5B-Instruct Q4_K_M (Apache-2.0) served under the default seccomp filter — preload, ready/status/list, non-streaming + streaming inference, mid-stream client cancellation, unload/reload/invalid loads, wrong/empty-token rejection, graceful SIGINT exit 0, kernel receipt `Seccomp: 2`. Closes SECURITY.md Known Gap 2 (sandboxed-inference smoke test).
- Sandbox startup receipts now log as `sandbox_applied` (Info) instead of being mislabeled `sandbox_violation` (Critical).
- Daemon sandbox activation (SECURITY.md Known Gap 1 closed): `gg-core-cli serve` now applies the process sandbox at startup (after model preload, before the listener), controlled by `GG_CORE_SANDBOX` — `seccomp` (syscall allowlist, no privileges; Linux x86_64 default), `enforce` (adds cgroup v2 limits sized from `GG_CORE_MAX_TOTAL_MEMORY`; Windows Job Objects default), `off` (loud notice). A requested sandbox that cannot apply aborts startup. Two latent defects in the never-exercised seccomp filter were fixed on the way: the BPF jump offsets were constant (`jt:1`), so a match on anything but the final comparison fell through toward KILL — the filter would have killed the process on most allowed syscalls; and the hand-numbered allowlist had mislabels (281 "eventfd2" is epoll_pwait; 254 "epoll_create1" is inotify_add_watch) and lacked modern glibc/tokio essentials (accept4, fcntl, statx, clone3, rseq, eventfd2, epoll_create1, sigaltstack, bind, listen). The allowlist is now empirically derived (daemon straced across startup, every IPC surface, and shutdown), expressed as compile-checked `libc::SYS_*` constants, and tightened: `execve`, `fork`/`vfork`, `connect` (no outbound sockets) and base `ioctl` are denied. Live-validated: all IPC surfaces, 20 concurrent probes, thread spawn, and graceful shutdown under the filter. New `SandboxConfig.cgroup_enabled` for seccomp-only mode; non-x86_64 Linux now fails loud instead of silently installing an x86_64-numbered filter.
- Model integrity enforcement (SECURITY.md Known Gap 2 closed): when a sibling `manifest.json` declares a `sha256`, the model file's digest is now computed (streaming) and verified in `load_model_dispatch` **before any backend reads the file**; a mismatch fails the load loudly and emits a critical `ModelHashMismatch` security event (new `models/integrity.rs`; unit + authenticated-IPC tests incl. tamper-after-manifest and no-partial-state).
- Cleared RUSTSEC-2026-0386 (tokio `AsyncFd` I/O-safety unsoundness): lockfile update tokio 1.49.0 → 1.54.1.
- Documented `deny.toml` ignores for the zip <9 extraction advisories (RUSTSEC-2026-0367/0368/0369/0429): zip enters only via candle-core (`onnx` feature), which reads `.npz` entries in-process; GG-CORE never calls the vulnerable `extract`-to-disk APIs. candle 0.11 caps zip at 8.x — drop the ignores on the next candle bump allowing zip ≥9.
- Fixed a probabilistic no-op in `security_audit::crypto_tests::swapped_ciphertext_fails` (~0.4%/run the swapped bytes were equal, leaving the ciphertext valid and false-failing CI — bit PR #123); the tamper is now deterministic.

### Added
- Build provenance: `build.rs` records the git commit (`GITHUB_SHA`/`GG_CORE_BUILD_COMMIT`/`git rev-parse`), build date and rustc version; `gg-core-cli version --json` reports them with the compiled feature set, protocol version and target triple, and `status --json`'s `version` block is populated (was `unknown`). The plain `version` line is unchanged.
- `scripts/rc-acceptance.sh` + a `qualify` job in the Release workflow: every packaged archive is installed on its own OS and must serve a real model before a tag can publish.
- `docs/integration/COREFORGE_PRODUCER_CONTRACT.md`: the verified consumer contract for CoreForge (managed daemon topology, IPC auth/protocol, lifecycle, schemas, readiness, errors, security configuration, consumer acceptance test, and the in-process crate path with its stated limits).

### Dependencies
- RustCrypto generation migration (supersedes/unblocks Dependabot #120 and #124): sha2 0.10 → 0.11, aes 0.8 → 0.9, aes-gcm 0.10 → 0.11, pbkdf2 0.12 → 0.13 (feature `simple` → `hmac`), and llama-cpp-2 0.1.69 → 0.1.158 — the llama-cpp-2 bump had been failing (#124) because its lockfile pulled the new sha2/digest generation into conflict with pbkdf2 0.12; moving the whole crypto stack forward resolves it. Code updated for `hybrid_array` (deprecated `from_slice` → `TryFrom`/`From`, nonce/key conversions by inference). All FIPS KATs, 113 lib security tests, and the 85-test security_audit suite pass unchanged — no algorithm or wire-format change. The GGUF backend was migrated to the new llama-cpp-2 vocab API (`model.vocab().is_eog/eos/tokenize/token_to_piece` replace the removed `is_eog_token`/`token_eos`/`str_to_token`/`token_to_piece`; `LlamaSampler::penalties` now takes `n_vocab`), preserving the old tokenize semantics (BOS added, special tokens parsed) and lossy-UTF-8 detokenization across token boundaries; the `encoding_rs` dependency is no longer needed and was removed. Stale `OpenSSL` license allowance dropped from `deny.toml`. Follow-up lockfile bump zeroize 1.8.2 → 1.9.0 / zeroize_derive 1.5.0 completes the group (supersedes Dependabot #120 entirely; #122/#123 lockfile bumps were already included).
- candle 0.8 → 0.11 (Dependabot #121; green across all CI legs; brings zip 1.1 → 8.6 with the extraction-advisory exposure noted above), aho-corasick 1.1.5, tempfile 3.27, GitHub Actions checkout/upload-artifact/setup-python major bumps (#117–#119).
- Declared `rust-version = "1.97"` (MSRV floor verified by a full local suite run on 1.97.0).
- Lockfile-only bump `futures` family 0.3.31/0.3.32 → 0.3.34, which no longer depends on `pin-utils` and so clears RUSTSEC-2026-0441 (`pin-utils` unmaintained, published 2026-11-05 — after the `deddcb6` qualification; the merge-commit preflight on `22e2c0e` failed cargo-deny on an otherwise identical tree). No source change; upstream 0.3.32–0.3.34 are soundness/leak fixes and deprecations only. The gate reopened and the bumped tree was requalified.

### Documentation
- Documentation reconciliation (issue #107): `SECURITY.md` rewritten against verified code reality — canonical product name, real supported-version line (0.9.x; no 1.x ever existed), self-assigned score removed, internal-assessment vs independent-audit distinction, FIPS self-tests vs certification clarified, real env vars (`CORE_AUTH_TOKEN`, `GG_CORE_SECURITY_INGRESS/EGRESS`; the documented `AUTH_TOKEN`/`SANDBOX_USER`/`RESOURCE_LIMITS` never existed), OWASP LLM coverage stated per-risk (6/10, not "full"), and an explicit **library-only vs daemon-enforced** maturity table disclosing that the sandbox is not yet applied by the standalone daemon and manifest `sha256` is not yet verified at load. `ROADMAP.md` rebuilt from code/CI/FEATURE_INDEX/BACKLOG with four maturity levels; GPU/MoE/A-B/deployment claims corrected from "complete" to library-only or not-real-yet; real release history. `docs/USAGE_GUIDE.md` rewritten: every Rust example now mirrors compile-tested `tests/doc_examples.rs` (CI-enforced, incl. the real `scan -> (is_safe, ...)` semantics), full real feature-flag and env-var tables, the v0.9.0 standalone surface documented, unevidenced performance multipliers and compatibility claims replaced with measured/evidence-based statements. `docs/IPC_PROTOCOL_SCHEMA.md` bumped to v0.9.0 (model lifecycle messages, sanitized-text streaming example, observability messages). Stale `docs/SYSTEM_STATE.md` retired in favor of live sources. CLI help banner now uses the canonical product name.

### CI / Tooling
- New `tests/doc_examples.rs` target keeps USAGE_GUIDE Rust snippets compiling and behaviorally accurate in CI.

## [0.9.0] - 2026-10-05

### Security / CI
- The `tests/security_audit/` penetration suite (auth attacks, sandbox boundaries, crypto, IPC fuzzing — 85 tests) is now actually built and run: it was rooted at `mod.rs`, which Cargo never discovers as a test target, so it had never compiled anywhere. Renamed to `main.rs` + compile-rot fixes (review C1).
- Machine-enforced supply-chain gate (review C3): `core-runtime/deny.toml` (RUSTSEC advisories, C.O.R.E. forbidden-dependency bans, license allowlist, registry pinning) + a `cargo-deny` CI job, `.github/dependabot.yml`, and `timeout-minutes` on all CI jobs. First run cleared RUSTSEC-2026-0204 (crossbeam-epoch), RUSTSEC-2026-0186 (memmap2), RUSTSEC-2026-0097 (rand) via lockfile updates.
- Tag-triggered release pipeline (`.github/workflows/release.yml`): a `vX.Y.Z` tag verifies the tree (fmt/clippy/tests + tag-version match), builds `gg-core-cli` + the `libgg_core` cdylib + `gg_core.h` on Linux/macOS/Windows (plus a GGUF-enabled Linux build), and publishes a GitHub Release with SHA-256 checksums and notes from this file.
- Removed the invalid `.cargo/config.toml` (its `[dependencies]` table was silently ignored by Cargo; the pin it intended never applied — review C2).
- Cleared the rustc 1.99 clippy lint wave across all CI legs (async-trait 0.1.92, `as_chunks` in the AVX2 kernels, unused test-glob imports).

### Added
- Standalone model bootstrap (B-41, issue #106 scopes 1–3): `serve --model <path> [--model-id <id>]` (repeatable, positional pairing) and `GG_CORE_PRELOAD_MODELS` preload models at startup through the canonical `validate_path → load_metadata → load_model_dispatch → ModelLifecycle::load` sequence (new shared `models/model_ops.rs`); a preload failure aborts startup. New authenticated IPC messages `model_load_request`/`model_unload_request` (+ responses) with handlers in `ipc/handler_models.rs` — load/unload require a session, paths stay confined to the base-path allowlist, failures are structured `success:false` responses with no partial registry state. New CLI commands `models load <path> [--id ID]`, `models unload <id>`, `models list --json` over a new authenticated single-connection exchange (`CliIpcClient::send_receive_authenticated`, token from `CORE_AUTH_TOKEN`). Standalone readiness now requires a loaded model (new `RuntimeConfig.health`; the daemon sets `require_model_loaded: true`, the embedded default is unchanged). The `serve` help no longer advertises unimplemented `--socket/--config/--auth-token` flags. 22 new tests (auth gating, IPC path traversal, no-partial-state, readiness flip, protocol round-trips, serve-arg parsing).
- Real ONNX embedding inference (B-ONNX-1, EvolveAI plan-v6.2 Phase 1): `OnnxEmbedder::load(model_dir, model_id, embedding_dim)` loads `<model_dir>/<model_id>/model.onnx` + sibling `tokenizer.json` offline (fail-closed on a missing/invalid model; `new()` remains the not-loaded state and still errors cleanly). The inference pipeline is now tokenize → `simple_eval` → **deterministic** hidden-state selection (`last_hidden_state` | single output | fail loud, mirroring the classifier's `logits` rule — replaces the nondeterministic `HashMap::values().next()`) → **attention-masked mean pooling** → **L2 normalization**. `InferenceInput::TextBatch` now embeds **every** item (previously first-item-only) via the additive `InferenceOutput::EmbeddingBatch(Vec<EmbeddingResult>)`; `Text` still returns `InferenceOutput::Embedding` (EvolveAI adapter source-compatible). `memory_usage()` reports the loaded model's initializer footprint and `unload()` zeroes it. Shared tensor helpers single-sourced in `engine/onnx/tensor_ops.rs` (classifier now imports from there). Tests run against a committed ~1 KB handcrafted ONNX fixture (`core-runtime/tests/fixtures/models/onnx/tiny-embedder/`, generated once by `scripts/gen_onnx_fixture.py` — nothing downloaded at build/test time): golden vector, unit L2 norm, determinism across loads, batch == per-item, fail-loud not-loaded/missing-model/no-feature paths, memory accounting.

### Fixed
- ONNX tokenization no longer fabricates an all-ones attention mask (B-ONNX-1 follow-up, caught by real-model validation against all-MiniLM-L6-v2): `OnnxTokenizer::encode` now returns the tokenizer's **own** attention mask alongside the ids (`TokenizedInput`), and `build_transformer_inputs` uses it verbatim. With a padding tokenizer (MiniLM's `tokenizer.json` pads to 128) the old fabricated mask made the model attend over ~120 `[PAD]` tokens and pooled them into every embedding — cosine similarities came out compressed and uniformly inflated (unrelated text at ~0.75 instead of ~-0.04). The embedder and classifier both take the fix; the hash fallback (never pads) emits an all-ones mask explicitly. The real-model e2e test (`load_and_embed_real_minilm`, skip-if-absent on the gitignored ~90 MB model) now golden-cross-checks the Rust pipeline against an onnxruntime + `tokenizers` reference: per-dimension agreement within 1e-3 and reference cosines reproduced within 0.02 (cat/kitten 0.6470, cat/quark -0.0415) — also confirming `candle_onnx::simple_eval` covers the full 19-op MiniLM graph (Erf/MatMul/Softmax/…).

### CI / Tooling
- The `advanced` feature is now linted and tested in CI (B-40): added an `advanced` leg to the `rust.yml` `features` matrix (`cargo clippy --features advanced --all-targets -- -D warnings` + `cargo test --features advanced`), closing the gap where advanced-gated code shipped unlinted. Fixed the 14 surfaced clippy lints (`div_ceil`, derivable `Default` on two enums, `map_or`→`is_none_or`, `Vec`-push→`resize`, and `#[allow(clippy::needless_range_loop)]` with justification on the 2-D SIMD quantize kernels where the index drives byte-offset arithmetic). The new leg immediately caught a latent unused-import under `advanced`-without-`gguf`.

### Security
- Added a grounded speculative-decoding security test (B-21d): `speculative_draft_pair_cannot_bypass_model_allowlist` confirms speculation introduces no path-traversal surface — `register_draft_pair` takes model *ids* (not paths) and performs no load, so a traversal-looking draft id is inert; the path allowlist is enforced upstream in `models/loader.rs` at load time (THREAT_MODEL §4.3). Corrected a phantom "THREAT_MODEL §12/§12.2" citation (the doc has §1–8) in the backlog and the speculative security-test header.

### Changed
- Speculative telemetry is now visible in `status` (B-21h): the executor emits the (previously dormant) Prometheus speculative counters each step, and `build_status` derives live speculative stats (draft/accepted/rejected counts, acceptance rate, mean accepted length) from the metrics snapshot the CLI already receives over IPC. Latency / net-speedup / auto-disable are not on the metrics channel and remain a follow-on (a dedicated IPC status field). F-64 added.

### Added
- Speculative KV-cache reuse for the GGUF backend (B-21f): a persistent `GgufSpeculativeSession` (via `self_cell`, owning `Arc<LlamaBackendInner>` + its borrowed `LlamaContext`) decodes the prompt once and, each step, decodes only the committed delta and rolls the speculative draft positions back out of the KV — removing the per-step full-context re-decode that made the wired speculative path net-slower and auto-disable. The session-backed `GgufTargetVerifier` and a new model-free **prompt-lookup draft** (`PromptLookupDraft`, n-gram copy from context) plug into the existing executor; a registered `register_draft_pair` still selects the classic model-based draft. All `#[cfg(feature="advanced")]`(+`gguf`)-gated and off by default. **Correctness is proven token-identical to single-model greedy** (the greedy target accepts a draft token iff it equals the target's argmax, so every committed token is the greedy token) — verified against the local qwen model. **Honest perf caveat**: a wall-clock >1× speedup is a GPU/batch phenomenon and is *not* demonstrable on CPU (a batched decode of _k_ draft tokens costs ≈ _k_× a single token, so speculation cannot win regardless of acceptance rate). On the CPU dev host the speculative path is slower than single-model; the real speedup demonstration is deferred to a GPU benchmark (B-21e). F-62/F-63 added.

### Documentation
- Adopted the canonical `docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md` onto `main` (B-21a) — the design of record that five FEATURE_INDEX rows (F-48/49/50/51/53) cited but which existed only on the unmerged PR #59 branch. Reconciled to the built reality: status Proposed → Accepted-implemented-(dormant), with an Implementation Status & Consolidation section recording that the ADR-007 stack (#61–#68, sealed #87–#94) is built + sealed but not yet wired into `Runtime::infer`, and the confirmed v1/v2 → single-`adaptive_speculative` retirement sequence.
- Added `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` (B-13) — the code-grounded technical spec that `CLAUDE.md` and `docs/TANDEM_EXPERIMENTS_PROPOSAL.md` cited but which did not exist (C.O.R.E. principles, security boundaries, module map, the secure `Runtime::infer` path, GGUF/ONNX dispatch, scheduler/memory, consumable-dependency shape).
- Corrected two mis-stated `docs/FEATURE_INDEX.md` rows (B-14): F-45 (Veritas shim) cited `n/a` despite 14 inline tests — now cited; F-38 (sandbox) marked verified with a unix-gated/CI-verified note. `feature_index_verify` reports 60/60 verified.

### Added
- ONNX inference servable end-to-end: `core_model_load` selects the GGUF or ONNX backend from a sibling `manifest.json` (`load_model_dispatch`); ONNX embed/classify reachable through FFI/Python (closes #72 scope-3).
- Degraded-mode policy: intentional, explained degradation under resource pressure — over-budget prompts are context-reduced instead of hard-failing (closes #53).

### Changed
- Unified GGUF/ONNX behind a single `engine::Model` trait; the registry now holds `Arc<dyn Model>`.
- `sandbox/unix.rs` split for Section 4 Razor (behavior unchanged).

### CI / Tooling
- Added a `bench` CI job (B-34) that runs the CI-safe default-feature benches on every PR to `main`, failing on compile error / bench panic and uploading the criterion baseline — preventing benchmark rot. (It immediately caught a rotted `ipc_throughput` bench → B-39.)
- Added a `security_overhead` bench (B-35) quantifying the per-call `SecurityPipeline` tax every `Runtime::infer` pays: `scan_prompt` ~8.7 ns/byte, `sanitize_output` ~53 ns/byte (the dominant stage), both linear per call. Joins the CI `bench` job. The linear-per-call sanitize result confirms the streaming egress re-sanitize is O(n²) over pushes → B-36 armed.
- Repaired the rotted `ipc_throughput` bench (B-39): `fixture_to_request` now derives the prompt from the fixtures' `prompt_tokens` size ladder instead of a missing top-level `prompt` string, and the bench is re-added to the CI `bench` job (the gate that caught the rot in B-34).
- Added a `scheduler_queue_overhead` bench (B-37) measuring the async `RequestQueue` enqueue/dequeue tax (tokio `Mutex` + `Notify`) over the bare `BinaryHeap`. Result (measurement only, no code change): ~550–620 ns per roundtrip, depth-insensitive, ~250 ns/op amortized under batch drain — <0.1% of per-request inference latency, so the scheduler is confirmed not a hotspot and needs no optimization.

### Added
- **Adaptive speculative decoding is now wired into `Runtime::infer`** (B-21c) — the ADR-007 stack is no longer dormant. A config-gated branch in `InferenceEngine::run` runs a GGUF draft/target speculative decode when `AdaptiveSpeculativeConfig` is active and a draft pair is registered (`register_draft_pair`), otherwise falling through to single-model. Off by default. The security path is unchanged: prompt-injection scan and PII sanitize wrap `run` in the façade, so the speculative branch inherits both; rejected draft suffixes are never committed; a new executor + GGUF adapter + tests (incl. the rejected-suffix-never-committed invariant) back it. All `#[cfg(feature="advanced")]`-gated. NOTE: with the current GGUF backend (fresh context per step, no KV reuse) the wired path is correct but not yet faster (it will auto-disable) — the actual speedup is B-21f (KV-cache reuse).

### Changed (BREAKING, advanced-gated)
- Retired the v1 speculative decoder (`engine/speculative.rs`, B-21b-1). `speculative_v2` is now the single token-level implementation, re-exported under the canonical unsuffixed names `gg_core::engine::{DraftModel, TargetModel, VerifyResult, SpeculativeConfig, SpeculativeDecoder, SpeculativeStats}`; the GGUF adapter + backend were ported to v2's traits. All speculative code is `#[cfg(feature="advanced")]`-gated and dormant (not wired into `Runtime::infer`), so there is no runtime impact; consumers importing `gg_core::engine::speculative::*` (v1) must use the canonical re-exports. First consolidation step of ADR-007 (triple → double); `adaptive_speculative` becomes canonical in B-21c.
- Retired the v2 speculative decoder (`engine/speculative_v2.rs`, B-21b-2), completing ADR-007's double → single consolidation: `adaptive_speculative` (wired in B-21c) is now the sole speculative executor. The v2 token-level decoder (`SpeculativeDecoder` + `DraftModel`/`TargetModel` traits), its GGUF token adapter (`gguf/speculative.rs`, `GgufDraftModel`/`GgufTargetModel`), and their tests were deleted. The shared value types were relocated **verbatim** to a new `gg_core::engine::speculative_types` module and are still re-exported under the canonical unsuffixed names `gg_core::engine::{SpeculativeConfig, SpeculativeStats, VerifyResult}` (behavior of `tier_synergy`, `decode.rs`, and GGUF `verify_draft_tokens` is unchanged); the `DraftModel`/`TargetModel`/`SpeculativeDecoder` re-exports are removed. All `#[cfg(feature="advanced")]`-gated and dormant except the now-canonical adaptive path, so no runtime impact. F-18 removed (subsumed by F-61).

### CI / Tooling (cont.)
- Hardened the perf-regression gate with a ~1µs noise floor (B-34c): sub-microsecond benches (whose CI variance exceeds the 2× threshold) are reported but no longer fail the gate; benches ≥1µs still gate unchanged. Fixes a false-positive flake on `concurrent_resource_ops`.
- Added a run-over-run perf-regression gate (B-34b): the CI `bench` job caches the criterion baseline from `main` and, on each PR, restores it and fails if any tracked bench's median regresses beyond 2.0× (a deliberately generous gross-regression threshold, since the trimmed CI bench run is noisy). No committed absolute baseline (hardware-relative baselines are unsound); the comparison is same-runner-class run-over-run via `core-runtime/scripts/perf_gate.py`. This closes the optimization initiative's measurement + gating work.

### Fixed
- `PromptCache::find_prefix` was O(n²) — it re-hashed every prefix `tokens[..len]` from scratch for each length. It now does a single forward SHA256 pass (cloning the running hasher per prefix), making longest-prefix lookup O(n) with identical results (B-38). Confirmed by the new `prompt_cache_overhead` bench (flat throughput across 64/512/2048 tokens). The prompt cache is dormant (not yet wired into inference), so this removes a latent trap before it ships.

### Performance
- Streaming egress PII sanitizer is now O(n) per stream instead of O(n²) (B-36). Previously every generated token re-sanitized the entire accumulated buffer; it now caches the sanitized stable prefix and re-sanitizes only a bounded tail, rebasing the prefix at boundaries proven to split no PII match. Output is byte-identical to the previous whole-buffer sanitize (verified by a differential test against a whole-buffer reference + a one-shot oracle), and the release decision stays on sanitized text so internal-separator PII (e.g. credit-card numbers) is never split and leaked.

### Security / **BREAKING**
- **`Runtime::infer`/`infer_stream` is now the sole external inference entry point.** `InferenceEngine::{run, run_cancellable, run_cancellable_with_memory_limit, run_stream_sync}` are `pub(crate)` — a consumer can no longer bypass the `SecurityPipeline` (ingress scan + egress PII sanitize). Embedded consumers that called the raw engine must switch to `runtime.infer()` (see COREFORGE #538). `InferenceEngine`/`new` remain public.

## [0.8.2] - 2026-07-27

### Security & Dependency Hardening

Consolidates the security-chain wiring, the unified secure inference façade, the
CI feature matrix, and the dependency-advisory cleanup accumulated since 0.8.1
(PRs #71–#79). No breaking public API change.

#### Security

- **Security pipeline wired into production** (`src/security/pipeline.rs`, PR #73):
  ingress prompt-injection scan + egress PII sanitization now run on the live
  inference path instead of existing only in tests.
- **Unified secure inference façade** (`src/runtime_facade.rs`, PR #75):
  `Runtime::infer` / `infer_stream` route both the embedded (COREFORGE) and the
  consumable (FFI/Python) surfaces through the same scan → engine → sanitize path;
  fixed an FFI/Python enqueue-with-no-worker deadlock.

#### Dependencies (advisory cleanup)

- **pyo3 0.21 → 0.29** (PR #78): clears RUSTSEC-2026-0176 (high), RUSTSEC-2026-0177
  (medium), RUSTSEC-2025-0020 (low); `pyo3-asyncio-0-21` → `pyo3-async-runtimes`.
- **rand 0.8 → 0.9** (PR #79): final Dependabot item; `OsRng` migrated to the
  `TryRngCore::unwrap_err()` adapter, preserving CSPRNG + panic-on-entropy-failure
  semantics on the key/nonce generation path.
- **Dropped `atty`** (PR #76): `cbindgen` 0.26 → 0.28 removes the unmaintained
  `atty` build-time dependency (RUSTSEC-2024-0375/0378).

#### Added

- **Real ONNX classifier** (`src/engine/onnx/classifier.rs`, PR #77): candle-onnx
  `simple_eval` classifier with a pure `logits_to_classification` (softmax+argmax)
  helper and deterministic output selection.
- **CI feature matrix** (`.github/workflows/rust.yml`, PR #71): dedicated
  `features` legs building `gguf` / `onnx` / `ffi` / `python`, flushing out latent
  per-feature clippy/compile debt on the CI-invisible surfaces.

#### Verified

- ✅ Full matrix green under `-D warnings`: fmt + clippy + test across 3 OS, plus
  the gguf/onnx/ffi/python feature legs, CodeQL, and Analyze.
- ✅ All Dependabot advisories cleared as of this release.

---

## [0.8.1] - 2026-02-20

### E2E Model Inference Verified

This release fixes critical bugs in the GGUF backend and adds verified E2E testing with real models.

#### Fixed

- **GGUF Batch Logits** (`src/engine/gguf/backend.rs`): Fixed `add_seq()` to compute logits only for the last token in the prompt batch, required for sampling
- **Sampler Index** (`src/engine/gguf/backend.rs`): Fixed `sampler.sample()` to use `-1` (last output) instead of sequence position, matching llama-cpp-2 API expectations

#### Added

- **Speculative Decoding for GGUF** (`src/engine/gguf/speculative.rs`): 2-3x CPU speedup via draft-verify loop
  - `GgufDraftModel`: Wrapper implementing `DraftModel` trait
  - `GgufTargetModel`: Wrapper implementing `TargetModel` trait
  - Backend methods: `generate_from_tokens()`, `verify_tokens()`, `eos_token()`
- **E2E Model Test** (`tests/e2e_model_test.rs`): Real model inference tests with Qwen 2.5 0.5B
  - `e2e_load_and_generate`: Batch generation test
  - `e2e_streaming_generation`: Token-by-token streaming test
  - `e2e_chat_messages`: Chat message formatting with system/user roles
  - `e2e_speculative_decoding`: Speculative decoding integration test
  - `e2e_performance_benchmark`: Throughput measurement (tok/s)
- **Test Scripts**: PowerShell build script for VS2022 + LLVM environment setup

#### Verified

- ✅ GGUF model loading (Qwen 2.5 0.5B, 463 MiB, Q4_K)
- ✅ Batch generation (~40 tok/s on CPU release, ~21 tok/s debug)
- ✅ Streaming generation (20 tokens via async channel)
- ✅ Chat messages with role formatting
- ✅ Flash Attention enabled automatically
- ✅ Memory usage: 435 MiB model + 299 MiB compute + 6 MiB KV cache

#### Benchmark Hardware

- CPU: Intel Core i7-7700K (4c/8t @ 4.2 GHz)
- RAM: 32 GB DDR4-2400
- OS: Windows 10 x64
- Build: Release with `lto = "thin"`, `codegen-units = 1`

---

## [0.8.0] - 2026-02-19

### GG-CORE Rebrand & Extension Point Architecture

This release rebrands from "Veritas SPARK" to "GG-CORE" (Greatest Good - Contained Offline Restricted Execution) and introduces the extension point architecture for commercial multi-tenant features.

#### Added

- **Request Shim Interface** (`src/shim/mod.rs`): Extension point for commercial features
  - `RequestInterceptor` trait for rate limiting, priority tagging, tenant context
  - `PassthroughInterceptor` default no-op implementation
  - `InterceptResult` and `InterceptError` types for interception results
- **Open Core Architecture**: Clear separation between OSS runtime and commercial extensions
  - GG-CORE OSS: Apache 2.0 licensed core runtime
  - GG-CORE Nexus: Commercial extension point (separate repo)

#### Changed

- **Complete Rebrand**: All references updated from Veritas SPARK to GG-CORE
  - `veritas-spark` → `gg-core` (crate name, CLI, socket paths)
  - `VERITAS_SPARK_*` → `GG_CORE_*` (environment variables)
  - Updated all documentation, comments, and branding

#### Philosophy

GG-CORE adopts triage principles ("Greatest Good for the Greatest Number"):
- **C.O.R.E.**: Contained, Offline, Restricted, Execution
- Resource-aware, multi-tenant AI that prioritizes system stability
- Extension points for commercial tiered service models

---

## [0.7.0] - 2026-02-19

### Streaming Inference

This release introduces real token-by-token streaming inference via IPC.

#### Added

- **Streaming Inference**: Token-by-token streaming via IPC with `stream: true` parameter
- **Mid-Stream Cancellation**: Cancel active streaming requests with `CancelRequest` message
- **CLI `infer` Command**: New CLI command for direct inference
  - `gg-core infer --model <MODEL> --prompt <PROMPT>` - Single response
  - `gg-core infer --model <MODEL> --prompt <PROMPT> --stream` - Streaming output
- **IpcStreamBridge**: New adapter for sending streaming chunks to IPC clients
- **StreamChunk.text Field**: Optional decoded text field for client display

#### Changed

- **E2E Test Scripts**: Updated to include streaming verification (steps 5-7)

#### Wire Protocol

New streaming protocol (backward compatible):

```json
// Request with stream: true
{ "type": "inference_request", "request_id": 1, "model_id": "...", "prompt": "...", "parameters": { "stream": true } }

// Multiple response chunks
{ "type": "stream_chunk", "request_id": 1, "token": 15496, "text": "Hello", "is_final": false }
{ "type": "stream_chunk", "request_id": 1, "token": 198, "text": "!", "is_final": true }

// Cancel request
{ "type": "cancel_request", "request_id": 1 }
```

#### Internal

- `process_streaming()` in handler.rs for streaming inference coordination
- `run_stream_sync()` for blocking task integration
- Split read/write connection handling in server.rs
- CancellationToken integration for mid-stream abort

---

## [0.6.7] - 2026-02-19

### Production Safety Fixes

This release focuses on production safety and fail-fast behavior for the COREFORGE integration.

#### Fixed

- **Flash Attention Placeholder**: CUDA and Metal implementations now return explicit errors instead of zero vectors when kernel not implemented
- **Tokenizer Stub Behavior**: `encode()` and `decode()` now return `TokenizerError::NotLoaded` instead of silently returning empty results
- **Handler Metrics**: Fixed hardcoded `ModelHandle::new(0)` - now uses proper model lookup for metrics attribution
- **Telemetry Integration**: Handler now calls `telemetry::record_request_success()` and `record_request_failure()` for Prometheus-compatible metrics
- **FFI Streaming**: Updated to use model_id lookup; token-based API now fails fast with deprecation message
- **Benchmark Protocol**: Updated IPC throughput and scheduler benchmarks to use v0.6.5 text-based protocol

#### Added

- `InferenceEngine::get_handle()` method for model_id to ModelHandle resolution
- 8 new tests for InferenceEngine and InferenceParams validation
- Explicit version roadmap comments for unimplemented status --json fields (v0.7.0+)

#### Changed

- Tokenizer tests updated to expect `NotLoaded` errors instead of empty results
- Prompt fixtures updated to use text-based `prompt` field instead of `prompt_tokens`

### Breaking Changes

- FFI streaming with token arrays now returns `InvalidParams` error
- Stub tokenizer operations now fail instead of returning empty values

---

## [0.6.5] - 2026-02-18

### Text-Based IPC Protocol

- Eliminated mock data paths
- Changed IPC protocol from tokenized to text-based prompts
- Added chaos testing infrastructure

---

## [0.6.0] - 2026-02-17

### Functional GGUF Backend

- Functional GGUF inference via llama-cpp-2
- IPC server implementation
- Chaos testing framework

---

Copyright 2024-2026 GG-CORE Contributors
