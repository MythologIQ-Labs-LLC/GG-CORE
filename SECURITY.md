# Security Policy

**GG-CORE** (Greatest Good - Contained Offline Restricted Execution) is a
security-first, offline inference runtime. This document states the security
posture honestly: what is implemented and enforced, what exists as a library
but is not yet wired into every surface, and what is planned. Every claim
here is checkable against the source; file references are given where a claim
is load-bearing.

---

## Security Posture

| Metric | Value |
| --- | --- |
| Assessment basis | Internal review only — **no independent audit has been performed** |
| Security tests | ~285 across `src/security/` (113), `tests/security_*` (79), `tests/security_audit/` penetration suite (85), auth + façade suites (8) |
| OWASP LLM Top 10 (2023 v1.1) | Partial — 6 of 10 risks addressed (see table) |
| Supply-chain gate | `cargo-deny` in CI: RUSTSEC advisories, dependency bans, license allowlist |
| License | Apache 2.0 |

Prior versions of this document carried a self-assigned numeric score; it has
been removed. A score is meaningful only from an independent assessment,
which remains planned (see Roadmap).

## Supported Versions

| Version | Supported | Notes |
| --- | --- | --- |
| 0.9.x | Yes | Current release line (first line published through the release pipeline) |
| ≤ 0.8.x | No | Pre-release-pipeline development tags (v0.6.5, v0.7.0, v0.8.2) |

There has never been a 1.x release.

## Reporting a Vulnerability

Use **GitHub private vulnerability reporting** on this repository
(Security → Report a vulnerability). Do not open a public issue for
security reports.

- Acknowledgement target: 7 days.
- Coordinated disclosure: we ask for a reasonable embargo while a fix ships.

## Security Features — Honest Maturity

**Enforced by default on the serving path** (ingress scan → inference →
egress sanitize; `Runtime::infer`/`infer_stream` is the sole external
inference path since v0.8.x):

| Feature | Detail | Evidence |
| --- | --- | --- |
| Prompt-injection detection | 55 base patterns + 5 high-risk + 5 context pairs; zero-width stripping; blocking on by default | `security/prompt_injection.rs` |
| PII detection/redaction | 13 PII types from 23 regexes; NFKC normalization; Luhn/SSN structural checks (other types are pattern-only) | `security/pii_detector.rs`, `pii_patterns.rs` |
| Streaming egress sanitization | In-runtime detokenization; windowed sanitizer with holdback — raw tokens never leave the runtime | `security/stream_sanitizer.rs` |
| IPC authentication | SHA-256 token (constant-time compare), per-session caps (1000 req/min), brute-force lockout (5 failures → 30 s); inference, cancel and model load/unload require an authenticated session bound to the connection that performed the handshake. The daemon **refuses to start with an empty token** unless `GG_CORE_ALLOW_INSECURE_NO_AUTH=1` (dev-only, loudly logged). Health, metrics and model *listing* are deliberately unauthenticated orchestrator probes. The shipped CLI performs the handshake for `infer`, `models load/unload` | `ipc/auth.rs`, `ipc/auth_session.rs`, `src/auth_boot.rs`, `cli/ipc_client_auth.rs` |
| IPC transport boundary | Unix: socket file created `0600` (owner-only) immediately after bind. Windows: the first pipe instance is claimed with `first_pipe_instance(true)` so a pre-existing pipe of the same name (squatting/impersonation) fails startup; remote clients rejected; default pipe DACL grants duplex access only to the creator, Administrators and SYSTEM | `ipc/listener.rs` |
| Path confinement | Model loads restricted to `models/`/`tokenizers/` under the configured base path. Lexical: NUL bytes and traversal rejected. **Physical**: an existing entry must *resolve* (symlinks, linked directories, other indirection) under the real path of an allowed root — a link planted inside `models/` that points elsewhere is refused and logged as `path_traversal_attempt`; rejections never echo the link target. A symlinked `models/` root itself is honored. Model files are opened without following a final-component symlink (`O_NOFOLLOW`) for hashing and mmap, so a link swapped in after validation is refused rather than read | `models/loader.rs::validate_path`, `models/containment.rs`, `models/integrity.rs::open_nofollow` |
| Model integrity | A sibling `manifest.json` that **exists must parse and validate** (64-hex `sha256`, non-empty id/capabilities) or the load fails — a malformed manifest never degrades to an unverified load. A valid manifest's `sha256` is verified before any backend reads the file, and the hashed file's identity (device+inode on Unix, volume serial+file index on Windows, plus size) is re-checked after the backend loaded by path; a rename/symlink substitution in between fails the load and nothing is registered. Models with no manifest (the ordinary GGUF case) have nothing declared to verify. Not covered: an in-place rewrite of an already-mapped file by a writer with access to the models directory (that is the operator trust boundary) | `models/backend_dispatch.rs`, `models/integrity.rs` |
| Daemon process sandbox | `serve` applies the sandbox at startup, after preload, before the listener. Linux x86_64 default: seccomp syscall allowlist (empirically derived from the serving daemon under strace; denies `execve`, `fork`, `connect` — no outbound sockets — and base `ioctl`; unknown syscall kills the process). `GG_CORE_SANDBOX=enforce` adds cgroup v2 memory limits (needs privileges). **Windows default: a Job Object** that caps job memory (`GG_CORE_MAX_TOTAL_MEMORY`), limits the job to a single active process (no child process creation), kills the job on handle close and dies on unhandled exceptions — it is a resource/containment limit, **not** a syscall filter and it does not block network or filesystem access. **macOS and other platforms: `off` by default with a loud notice — no daemon sandbox is enforced there.** A requested sandbox that cannot apply **aborts startup**. Linux x86_64 validated live with a real GGUF model under the default filter: preload, ready/status/list, non-streaming and streaming inference, mid-stream client cancellation, unload/reload/invalid loads, and graceful SIGINT exit 0 (kernel receipt `Seccomp: 2` in `/proc/<pid>/status`; no SIGSYS). If a field workload hits an unlisted syscall (SIGSYS kill, visible in `dmesg`), `GG_CORE_SANDBOX=off` is the documented escape hatch; file an issue | `sandbox/`, `src/sandbox_boot.rs` |
| Input validation | Text ≤ 65,536 bytes, ≤ 4096 input tokens, batch ≤ 32 | `engine/input.rs` |
| Resource limits | Per-call / total-memory / concurrency gates | `memory/limits.rs`, env-configurable |
| Security event log | 15 `SecurityEvent` variants via structured tracing (incl. `sandbox_applied` startup receipt and critical `insecure_configuration`) | `telemetry/security_log.rs` |

**Implemented as libraries, NOT yet applied by the standalone daemon** —
these are real, tested code paths that embedding hosts can invoke, but
`gg-core-cli serve` does not currently activate them:

| Feature | State | Gap |
| --- | --- | --- |
| Model encryption (AES-256-GCM, PBKDF2 keys, installation salt, key zeroing, nonce-reuse detection with 10,000-nonce history) | Library + 39 tests | The model load path never decrypts; `enable_model_encryption` is not read by the loader |

**FIPS 140-3**: the runtime executes power-on cryptographic self-tests at
daemon startup and refuses to start if they fail. This is *FIPS-informed
self-testing*. **GG-CORE holds no FIPS 140-3 validation or certification.**

### OWASP LLM Top 10 (2023 v1.1) — actual coverage

| Risk | Coverage |
| --- | --- |
| LLM01 Prompt Injection | Detection + blocking + zero-width stripping |
| LLM02 Insecure Output Handling | PII sanitization, streaming holdback |
| LLM03 Training Data Poisoning | Out of scope (inference-only runtime) |
| LLM04 Model DoS | Rate limits, resource gates, degraded-mode policy |
| LLM05 Supply Chain | Partial: `cargo-deny` advisory/ban gate for *code* dependencies; manifest-declared model hashes verified at load (no-manifest models remain unverified) |
| LLM06 Sensitive Info Disclosure | PII detection/redaction, NFKC |
| LLM07 Insecure Plugin Design | Out of scope (no plugin system by design) |
| LLM08 Excessive Agency | Out of scope by architecture (pure compute, no tool/data authority) |
| LLM09 Overreliance | Not addressed (consumer concern) |
| LLM10 Model Theft | Partial: path confinement + encryption library; sandbox not daemon-applied |

## Configuration

Security behavior is environment-driven (`src/security/mod.rs`, `src/config.rs`):

| Variable | Values | Default |
| --- | --- | --- |
| `GG_CORE_SECURITY_INGRESS` | `block` / `detect` / `off` | `block` |
| `GG_CORE_SECURITY_EGRESS` | `redact` / `off` | `redact` |
| `GG_CORE_SANDBOX` | `enforce` / `seccomp` / `off` | `seccomp` (Linux x86_64), `enforce` (Windows), `off` elsewhere |
| `CORE_AUTH_TOKEN` | shared IPC auth token (daemon + CLI) | **required by `serve`** — an empty token aborts startup (16+ bytes recommended; shorter warns) |
| `GG_CORE_ALLOW_INSECURE_NO_AUTH` | `1` = allow `serve` with an empty token (development only; logs a critical `insecure_configuration` event and prints a banner on every start) | unset |
| `GG_CORE_MAX_MEMORY_PER_CALL` / `GG_CORE_MAX_TOTAL_MEMORY` / `GG_CORE_MAX_CONCURRENT` | resource gates | see `config.rs` |

Programmatic embedding uses `security::SecurityConfig`
(`enable_prompt_injection_detection`, `block_prompt_injection`,
`enable_pii_detection`, `redact_pii`, `enable_model_encryption`,
`encryption_key`). Variable and field names in earlier versions of this
document (`AUTH_TOKEN`, `SANDBOX_USER`, `RESOURCE_LIMITS`,
`enable_prompt_injection_filter`, …) never existed.

## Verification

```bash
# Full security surface
cargo test --test security_audit            # 85 penetration tests
cargo test --lib security                   # unit suites
cargo test --test security_path_traversal_test
cargo test --test security_sandbox_escape_test   # library-level sandbox tests

# Supply chain
cargo deny check                            # advisories, bans, licenses, sources
```

CI runs all of the above on every push/PR (`rust.yml`), including the
penetration suite on Linux, macOS, and Windows.

## Known Gaps (tracked, not hidden)

1. **Model encryption wiring** — connect `ModelEncryption` to the load path
   behind its config flag.
2. **No daemon sandbox on macOS** — `serve` runs with `GG_CORE_SANDBOX=off`
   semantics there (loud notice). Windows Job Objects limit resources and
   process creation but do not filter syscalls or network access.
3. **Independent audit** — planned post-1.0 (see ROADMAP).

## Security Changelog

| Release | Security-relevant changes |
| --- | --- |
| 0.9.0 | security_audit penetration suite activated in CI (85 tests); cargo-deny supply-chain gate; RUSTSEC-2026-0204/0186/0097 cleared; authenticated IPC model load/unload; model-gated readiness |
| 0.8.2 | pyo3 0.29 (3 RUSTSEC advisories), rand 0.9 migration; KV-cache cross-sequence isolation redesign; NUL-byte path rejection; streaming egress sanitization; secure façade as sole inference path |
| ≤ 0.8.1 | see CHANGELOG.md |
