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
| IPC authentication | SHA-256 token (constant-time compare), per-session caps (1000 req/min), brute-force lockout (5 failures → 30 s); model load/unload require an authenticated session | `ipc/auth.rs`, `ipc/auth_session.rs` |
| Path confinement | Model loads restricted to `models/`/`tokenizers/` under the configured base path; NUL bytes and traversal rejected lexically | `models/loader.rs::validate_path` |
| Input validation | Text ≤ 65,536 bytes, ≤ 4096 input tokens, batch ≤ 32 | `engine/input.rs` |
| Resource limits | Per-call / total-memory / concurrency gates | `memory/limits.rs`, env-configurable |
| Security event log | 13 `SecurityEvent` variants via structured tracing | `telemetry/security_log.rs` |

**Implemented as libraries, NOT yet applied by the standalone daemon** —
these are real, tested code paths that embedding hosts can invoke, but
`gg-core-cli serve` does not currently activate them:

| Feature | State | Gap |
| --- | --- | --- |
| Sandbox (Windows Job Objects; Linux cgroups v2 + seccomp-bpf, 49-syscall allowlist, kill-on-unknown) | Library + tests pass | `create_sandbox` is never called from the daemon startup path |
| Model encryption (AES-256-GCM, PBKDF2 keys, installation salt, key zeroing, nonce-reuse detection with 10,000-nonce history) | Library + 39 tests | The model load path never decrypts; `enable_model_encryption` is not read by the loader |
| Model hash verification | Manifest carries a `sha256` field | Only its length is validated; no digest is computed on load — **do not rely on manifest hashes for integrity today** |

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
| LLM05 Supply Chain | Partial: `cargo-deny` advisory/ban gate for *code* dependencies; model hash verification not yet enforced |
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
| `CORE_AUTH_TOKEN` | shared IPC auth token (daemon + CLI) | empty (only an empty-token handshake succeeds) |
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

1. **Daemon sandbox activation** — the standalone daemon should apply the
   sandbox it ships. Until then, deploy `gg-core-cli serve` under an external
   sandbox (systemd hardening, containers, Job Objects).
2. **Model hash enforcement** — compute and verify the manifest `sha256` at
   load; fail loud on mismatch.
3. **Model encryption wiring** — connect `ModelEncryption` to the load path
   behind its config flag.
4. **Independent audit** — planned post-1.0 (see ROADMAP).

## Security Changelog

| Release | Security-relevant changes |
| --- | --- |
| 0.9.0 | security_audit penetration suite activated in CI (85 tests); cargo-deny supply-chain gate; RUSTSEC-2026-0204/0186/0097 cleared; authenticated IPC model load/unload; model-gated readiness |
| 0.8.2 | pyo3 0.29 (3 RUSTSEC advisories), rand 0.9 migration; KV-cache cross-sequence isolation redesign; NUL-byte path rejection; streaming egress sanitization; secure façade as sole inference path |
| ≤ 0.8.1 | see CHANGELOG.md |
