# Threat Model - COREFORGE CORE Runtime

**Version:** 0.8.1
**Document Date:** 2026-02-20
**Classification:** Internal / Audit Preparation
**Framework:** STRIDE + Attack Trees

---

## 1. System Overview

### 1.1 Purpose

COREFORGE CORE Runtime is a sandboxed, offline inference engine that performs model execution only. It operates as a pure compute service with no authority over data, tools, or system actions.

### 1.2 Design Principles (C.O.R.E.)

| Principle | Implementation |
|-----------|----------------|
| **Contained** | Separate OS process, restricted user, seccomp/AppContainer |
| **Offline** | Zero network access (inbound/outbound blocked) |
| **Restricted** | IPC-only communication with authenticated callers |
| **Execution** | Pure compute, no business logic or decision authority |

### 1.3 Trust Boundaries

```
┌─────────────────────────────────────────────────────────────────┐
│                        HOST SYSTEM                              │
│                                                                 │
│  ┌──────────────────┐          ┌──────────────────────────────┐│
│  │  Control Plane   │◄─────────►│       CORE Runtime          ││
│  │  (Trusted)       │   IPC    │     (Sandboxed)              ││
│  │                  │          │  ┌─────────────────────────┐ ││
│  │  - Auth decision │          │  │ Trust Boundary 2        │ ││
│  │  - Data policy   │          │  │                         │ ││
│  │  - Tool auth     │          │  │  Inference Engine       │ ││
│  └──────────────────┘          │  │  - Model loading        │ ││
│           │                    │  │  - Token generation     │ ││
│   Trust Boundary 1             │  │  - KV cache             │ ││
│           │                    │  └─────────────────────────┘ ││
│           ▼                    │                              ││
│  ┌──────────────────┐          │  Filesystem (Read-only):     ││
│  │  User Input      │          │  - models/                   ││
│  │  (Untrusted)     │          │  - tokenizers/               ││
│  └──────────────────┘          │                              ││
│                                └──────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

**Trust Boundary 1 (TB1):** IPC channel between Control Plane and CORE
**Trust Boundary 2 (TB2):** Model execution environment

---

## 2. Protected Assets

| Asset | Sensitivity | Location | Protection Goal |
|-------|-------------|----------|-----------------|
| Model weights | HIGH | `models/` | Confidentiality, Integrity |
| User prompts | HIGH | IPC transit | Confidentiality |
| Inference outputs | HIGH | IPC transit | Integrity |
| Session tokens | HIGH | Memory | Confidentiality |
| Encryption keys | CRITICAL | Memory | Confidentiality |
| KV cache | MEDIUM | Memory | Availability |
| Telemetry data | LOW | Memory/disk | Integrity |

---

## 3. Threat Actors

### 3.1 External Threat Actors

| Actor | Capability | Motivation | Access |
|-------|------------|------------|--------|
| **Malicious User** | Crafted prompts | Data exfiltration, model abuse | Indirect via prompts |
| **Network Attacker** | N/A (offline) | N/A | None |
| **Supply Chain** | Compromised deps | Backdoor insertion | Build-time |

### 3.2 Internal Threat Actors

| Actor | Capability | Motivation | Access |
|-------|------------|------------|--------|
| **Rogue Process** | Local process | Privilege escalation | Same host |
| **Compromised Control Plane** | IPC access | Full system compromise | Direct IPC |

---

## 4. Attack Surfaces

### 4.1 IPC Protocol (Primary Attack Surface)

**Entry Points:**
- `decode_message()` - JSON parsing of incoming messages
- `decode_message_binary()` - Bincode deserialization
- Session handshake flow
- Streaming response channel

**Security Controls:**
- Size limits enforced (16MB max message)
- Authentication required for all requests
- Constant-time token comparison
- Rate limiting on authentication failures
- Protocol versioning

**Fuzz Targets:** `fuzz_ipc_json`, `fuzz_ipc_binary`

### 4.2 Prompt Processing

**Entry Points:**
- `InferenceRequest.prompt` field
- System prompt injection points
- Token streaming output

**Security Controls:**
- Prompt injection detection (Aho-Corasick pattern matching)
- Risk scoring (0-100 scale)
- Configurable blocking threshold
- PII detection and redaction
- Output sanitization

**Fuzz Targets:** `fuzz_prompt_injection`, `fuzz_pii_detection`, `fuzz_output_sanitizer`

### 4.3 Model Loading

**Entry Points:**
- Model file path specification
- GGUF/ONNX file parsing
- Tokenizer vocabulary loading

**Security Controls:**
- Path validation (no traversal)
- File format validation
- Size limits on model files
- Encrypted model support (AES-256-GCM)
- PBKDF2 key derivation (100K iterations)

### 4.4 Memory Management

**Entry Points:**
- KV cache allocation
- Token buffer management
- GPU memory allocation

**Security Controls:**
- Bounded memory pools
- Mutex poison recovery (graceful degradation)
- No unbounded allocations from untrusted input

---

## 5. STRIDE Analysis

### 5.1 Spoofing

| Threat | Impact | Mitigation | Status |
|--------|--------|------------|--------|
| Session hijacking | HIGH | CSPRNG session IDs, constant-time comparison | MITIGATED |
| Replay attacks | MEDIUM | Session timeouts, request IDs | MITIGATED |
| IPC impersonation | HIGH | Named pipe authentication, process verification | MITIGATED |

### 5.2 Tampering

| Threat | Impact | Mitigation | Status |
|--------|--------|------------|--------|
| Model file modification | HIGH | AES-256-GCM with auth tags | MITIGATED |
| IPC message tampering | HIGH | Message authentication (bincode CRC) | PARTIAL |
| Memory corruption | CRITICAL | Rust memory safety, bounds checking | MITIGATED |

### 5.3 Repudiation

| Threat | Impact | Mitigation | Status |
|--------|--------|------------|--------|
| Untracked model operations | MEDIUM | Audit logging module | MITIGATED |
| Missing access logs | MEDIUM | Security event logging | MITIGATED |

### 5.4 Information Disclosure

| Threat | Impact | Mitigation | Status |
|--------|--------|------------|--------|
| Prompt leakage | HIGH | No persistent storage, memory clearing | MITIGATED |
| Model extraction | HIGH | Process isolation, no network | MITIGATED |
| PII in outputs | MEDIUM | PII detection and redaction | MITIGATED |
| System prompt extraction | MEDIUM | Prompt injection filtering | MITIGATED |
| Timing side-channels | LOW | Constant-time auth comparison | MITIGATED |

### 5.5 Denial of Service

| Threat | Impact | Mitigation | Status |
|--------|--------|------------|--------|
| Message size bomb | HIGH | 16MB size limit, atomic validation | MITIGATED |
| Prompt length attack | MEDIUM | Token limits, configurable max | MITIGATED |
| Auth brute-force | MEDIUM | Rate limiting (5 attempts/30s block) | MITIGATED |
| Thread pool exhaustion | MEDIUM | Bounded thread pool, priority queuing | MITIGATED |
| KV cache exhaustion | MEDIUM | LRU eviction, bounded cache size | MITIGATED |

### 5.6 Elevation of Privilege

| Threat | Impact | Mitigation | Status |
|--------|--------|------------|--------|
| Sandbox escape | CRITICAL | seccomp/AppContainer, minimal syscalls | MITIGATED |
| File system traversal | HIGH | Path validation, allowlist directories | MITIGATED |
| Network access | HIGH | Zero network (blocked at OS level) | MITIGATED |
| Arbitrary code exec | CRITICAL | No plugin/script loading, no eval | MITIGATED |

---

## 6. Attack Trees

### 6.1 Model Extraction Attack

```
Goal: Extract proprietary model weights
├── Via Network Exfiltration
│   └── BLOCKED: No network access (OS-level deny)
├── Via File System Access
│   ├── Direct read of models/
│   │   └── BLOCKED: Sandboxed filesystem access
│   └── Path traversal
│       └── BLOCKED: Path validation, no ".." allowed
├── Via Memory Dump
│   ├── Process memory access
│   │   └── BLOCKED: Process isolation, restricted user
│   └── Core dump analysis
│       └── BLOCKED: Core dumps disabled in production
└── Via IPC Smuggling
    ├── Encode model in responses
    │   └── MITIGATED: Output size limits, sanitization
    └── Timing/side-channel
        └── PARTIAL: Constant-time ops for auth only
```

### 6.2 Prompt Injection Attack

```
Goal: Override system instructions or extract system prompt
├── Direct Instruction Override
│   ├── "Ignore previous instructions"
│   │   └── BLOCKED: Pattern matching, risk score
│   └── "You are now DAN"
│       └── BLOCKED: High-risk pattern detection
├── Indirect Injection
│   ├── Delimiter attacks (---, ```)
│   │   └── MITIGATED: Delimiter pattern matching
│   └── Encoding attacks (base64, rot13)
│       └── MITIGATED: Encoding pattern detection
├── System Prompt Extraction
│   ├── "Repeat your instructions"
│   │   └── BLOCKED: Extraction pattern matching
│   └── "What is your system prompt"
│       └── BLOCKED: Pattern matching
└── Context Manipulation
    ├── "This is only a test"
    │   └── MITIGATED: Context pattern detection
    └── "Hypothetically..."
        └── MITIGATED: Pattern matching
```

---

## 7. Security Controls Summary

### 7.1 Cryptographic Controls

| Control | Algorithm | Parameters | Standard |
|---------|-----------|------------|----------|
| Model encryption | AES-256-GCM | 96-bit nonce, 128-bit tag | NIST SP 800-38D |
| Key derivation | PBKDF2-HMAC-SHA256 | 100,000 iterations | OWASP minimum |
| Session ID generation | CSPRNG | 256-bit | OS random source |
| Token comparison | Constant-time | subtle crate | Timing-safe |

### 7.2 Access Controls

| Control | Implementation | Scope |
|---------|----------------|-------|
| IPC authentication | Session tokens + rate limiting | All requests |
| Filesystem access | Read: `models/`, `tokenizers/`. Write: `temp/`, `cache/` | Process-level |
| Network access | Deny all (OS-level) | Process-level |
| Memory access | Rust ownership + bounds checking | Language-level |

### 7.3 Input Validation

| Input | Validation | Location |
|-------|------------|----------|
| IPC messages | Size limits, format validation | `ipc/protocol.rs` |
| File paths | No traversal, allowlist | `models/loader.rs` |
| Prompts | Injection detection, PII scan | `security/prompt_injection.rs` |
| K8s CRDs | Path, image, model ID validation | `k8s/types.rs` |

### 7.4 Runtime Protection

| Control | Implementation | Recovery |
|---------|----------------|----------|
| Mutex poisoning | `unwrap_or_else(poison.into_inner())` | Graceful degradation |
| Thread panics | Catch unwind in thread pool | Worker respawn |
| Memory exhaustion | Bounded allocations, LRU eviction | Request rejection |

---

## 8. Residual Risks

### 8.1 Accepted Risks

| Risk | Severity | Rationale |
|------|----------|-----------|
| Side-channel attacks on inference | LOW | Constant-time only for auth; inference timing visible |
| Model format vulnerabilities | MEDIUM | Depends on upstream GGUF/ONNX parser security |
| Sophisticated prompt injection | MEDIUM | Pattern-based detection has bypass potential |

### 8.2 Risks Requiring External Validation

| Risk | Recommended Testing |
|------|---------------------|
| Sandbox escape vectors | Penetration testing |
| Memory corruption in unsafe blocks | Fuzzing + formal verification |
| Cryptographic implementation | Cryptographic audit |
| Supply chain dependencies | Dependency audit + SBOM |

---

## 9. Audit Recommendations

### 9.1 Priority Areas for External Audit

1. **IPC Protocol Parsing** (HIGH)
   - Files: `ipc/protocol.rs`, `ipc/handler.rs`
   - Focus: Deserialization safety, size validation, bounds checking

2. **Cryptographic Implementation** (HIGH)
   - Files: `security/encryption.rs`
   - Focus: Key derivation, nonce handling, error conditions

3. **Sandbox Boundaries** (HIGH)
   - Files: OS integration points
   - Focus: seccomp filters, AppContainer policies

4. **Unsafe Code Blocks** (MEDIUM)
   - Files: FFI boundaries, SIMD code
   - Focus: Memory safety invariants

5. **Prompt Injection Bypasses** (MEDIUM)
   - Files: `security/prompt_injection.rs`
   - Focus: Evasion techniques, Unicode normalization

### 9.2 Automated Testing Available

| Test Type | Location | Command |
|-----------|----------|---------|
| Unit tests | `src/**/tests.rs` | `cargo test` |
| Security tests | `tests/security_*.rs` | `cargo test --test security` |
| Fuzz tests | `fuzz/fuzz_targets/` | `cargo +nightly fuzz run <target>` |
| Benchmarks | `benches/` | `cargo bench` |

### 9.3 Documentation for Auditors

| Document | Location | Contents |
|----------|----------|----------|
| Concept | `docs/CONCEPT.md` | Design philosophy, anti-goals, security tax |
| Security Analysis | `docs/security/SECURITY_ANALYSIS_REPORT.md` | CVE remediations, test coverage |
| This Document | `docs/security/THREAT_MODEL.md` | Threat analysis, attack trees |
| Usage Guide | `docs/USAGE_GUIDE.md` | API reference and usage patterns |

---

## 10. Version History

| Version | Date | Changes |
|---------|------|---------|
| 0.6.1 | 2026-02-18 | SA-1 Audit: Enhanced trust boundary documentation, updated control matrix |
| 0.6.0 | 2026-02-17 | Initial threat model, fuzz targets added |

---

## 11. Appendix: Security Test Coverage

### 11.1 Test Counts by Category

| Category | Count | Coverage |
|----------|-------|----------|
| Encryption | 20+ | PBKDF2, AES-GCM, edge cases |
| Authentication | 12 | Auth flows, timing, sessions |
| IPC Protocol | 16 | Versioning, encoding, limits |
| Prompt Injection | 10 | Patterns, sanitization, performance |
| PII Detection | 8 | Detection, redaction, consistency |
| K8s Validation | 15+ | Input validation |
| **Total** | 430+ | Security-critical paths |

### 11.2 Fuzz Target Coverage

| Target | Functions Covered | Priority |
|--------|-------------------|----------|
| `fuzz_ipc_json` | `decode_message()` | HIGH |
| `fuzz_ipc_binary` | `decode_message_binary()` | HIGH |
| `fuzz_prompt_injection` | `scan()`, `sanitize()` | HIGH |
| `fuzz_pii_detection` | `detect()`, `redact()` | MEDIUM |
| `fuzz_output_sanitizer` | `sanitize()`, `validate_format()` | MEDIUM |

---

## 12. Speculative Decoding Threat Model (ADR-007)

**Scope:** `advanced` feature gate — `engine/adaptive_speculative/`, `engine/speculative.rs`,
`models/speculative_config.rs`, `models/tier_synergy_speculative.rs`.

**Version:** 0.8.2 | **Date:** 2026-07-08 | **Issue:** #67

### 12.1 Threat Summary

| ID | Title | STRIDE | Impact | Status |
|----|-------|--------|--------|--------|
| T1 | Draft model loading — malicious/corrupt file | Tampering | HIGH | MITIGATED |
| T2 | Target verification bypass — premature commit | Tampering | CRITICAL | MITIGATED |
| T3 | Telemetry leakage — PII in speculative stats | Information Disclosure | HIGH | MITIGATED |
| T4 | Incompatible tokenizer pairing | Tampering | HIGH | MITIGATED |
| T5 | Auto-disable evasion via adversarial input | Elevation of Privilege | MEDIUM | MITIGATED |

---

### 12.2 T1 — Draft Model Loading: Malicious or Corrupt File

**Description:** A draft model file placed in `models/` has been tampered with to contain
a malicious payload, an oversized tensor that exhausts GPU memory, or structurally corrupt
GGUF/ONNX data designed to exploit the parser.

**Attack Vector:** Supply-chain compromise of the model directory, or a rogue process with
write access to `models/` (outside the sandbox boundary — TB1). The draft model path is
specified at configuration time, before `GgufDraftModel::new` is called.

**Mitigation:**
- Model file path is validated against the `models/` allowlist (no `..` traversal).
- GGUF/ONNX parsers enforce tensor size bounds before allocation.
- AES-256-GCM authenticated encryption detects any byte-level modification.
- Draft model is loaded through the same `ModelLoader` path as the target model; no
  separate, unvalidated code path exists.

**Test Binding:** `security_speculative_test::t1_draft_model_path_enforces_allowlist`
(structural — verifies the load path shares the common loader).

---

### 12.3 T2 — Target Verification Bypass: Rejected Tokens Committed Before Verification

**Description:** An implementation bug or a crafted `VerificationResult` causes the executor
to emit draft tokens beyond `accepted_count` into the output stream before the target model
has confirmed them. The caller receives semantically invalid tokens that appear authoritative.

**Attack Vector:** A malformed `VerificationResult` (e.g., `accepted_count` larger than
the draft length, or a missing correction token after rejection) reaching the output
assembly logic in `VerificationResult::into_tokens`.

**Mitigation:**
- `VerificationResult::into_tokens` slices `draft` with `.take(accepted_count)`, which
  saturates at `draft.len()` — overflows are structurally impossible.
- Correction token (T+1 at rejection point) is appended only when `correction_token`
  is `Some`; tokens at positions `> accepted_count` are never read.
- The speculative step in `SpeculativeDecoder::accept_tokens` (`speculative.rs`) applies
  the same slice-then-append contract.

**Test Binding:** `security_speculative_test::t2_rejected_suffix_never_emitted`

---

### 12.4 T3 — Telemetry Leakage: Prompt Text or PII in Speculative Stats

**Description:** Speculative decoding statistics structs (counters, acceptance rates, timing)
accumulate numeric data over many requests. If prompt text or token identifiers that
reconstruct sensitive input are stored in these structs, they become an ambient PII store
that outlives the request.

**Attack Vector:** A future developer adds a `last_prompt: String` or `context_snapshot`
field to `AdaptiveSpeculativeConfig` or `SpeculativeStats` for debugging. The field is then
exposed via the telemetry subsystem or an IPC introspection call.

**Mitigation:**
- `AdaptiveSpeculativeConfig` contains only numeric and boolean configuration values —
  no `String`, `Vec<u8>`, or `Box<dyn Any>` fields.
- `SpeculativeStats` (`speculative_v2.rs`) stores only aggregate counters and
  timing durations — no per-request or per-token content.
- Field-level structural test (`t3_config_has_no_string_fields`) enforces this at compile
  time by exhaustive field enumeration; any added `String` field will cause a type mismatch
  in the test.

**Test Binding:** `security_speculative_test::t3_config_fields_contain_no_pii_types`

---

### 12.5 T4 — Incompatible Tokenizer Pairing: Misaligned Verification

**Description:** A draft model and target model that use different tokenizer vocabularies
(e.g., a Mistral-3 draft paired with a Llama-3 target) produce token ids that are
semantically misaligned. The target model verifies draft tokens against the wrong
distribution, silently producing corrupt output that appears accepted.

**Attack Vector:** `TierSpeculativePlan::select` is called with two available tiers that
appear compatible by tier name but whose underlying tokenizer families differ. The
`CompatibilityCheck` defaults to `Unknown`, and an operator skips the required runtime
verification step.

**Mitigation:**
- `TierSpeculativePlan` carries a `CompatibilityCheck` field. A `FamilyMismatch` result
  must cause the caller to fall back to `is_speculative = false`.
- When `config.enabled = false` or `config.is_active()` returns `false`, `select` returns
  a single-model plan with `is_speculative = false`, removing the pairing risk entirely.
- `CompatibilityCheck::Unknown` is treated conservatively in the executor (requires
  explicit opt-in from the runtime layer before a speculative step proceeds).

**Test Binding:** `security_speculative_test::t4_disabled_config_yields_single_model_plan`

---

### 12.6 T5 — Auto-Disable Evasion: Adversarial Input Prevents Self-Disable

**Description:** The `AdaptiveVerificationScheduler` monitors rolling acceptance rate and
calls `VerificationPlan::fallback()` when the speedup drops below `auto_disable_threshold`.
An adversary controlling the prompt could craft inputs that keep the acceptance rate
artificially high — suppressing auto-disable while the draft model continues to generate
plausible-but-wrong completions for non-adversarial requests.

**Attack Vector:** Adversarial prompts are structured such that the draft model's distribution
closely mirrors the target's, maintaining a high acceptance rate. The scheduler never fires
`auto_disable`, keeping speculation enabled even when the quality guarantee would otherwise
require single-model decoding.

**Mitigation:**
- Auto-disable threshold (`auto_disable_threshold`) defaults to `1.05` (requiring at least
  5% net speedup). The threshold is configured at deployment time by the operator and is
  not overridable by request content.
- `AdaptiveVerificationScheduler::plan` evaluates `should_auto_disable` before any
  window computation; `is_active()` (which checks both `enabled` and mode) is the first
  gate. When the mode is `AdaptiveMode::Disabled`, `mode_multiplier` returns `0.0` and
  the window is clamped to zero, forcing `VerificationPlan::fallback()`.
- Fallback returns `window = 0`, preventing any unchecked draft tokens from being emitted.

**Test Binding:** `security_speculative_test::t5_auto_disable_fires_below_threshold`,
`security_speculative_test::t5_fallback_plan_has_zero_window`

---

### 12.7 Speculative Decoding Attack Tree

```
Goal: Emit unverified tokens to the caller
├── T2: accepted_count overflow
│   └── BLOCKED: .take(accepted_count) saturates at draft.len()
├── T2: correction token skipped
│   └── BLOCKED: into_tokens() appends correction unconditionally when Some(_)
├── T4: incompatible tokenizer bypasses verification
│   └── MITIGATED: CompatibilityCheck::Unknown; config.enabled=false → single-model
├── T5: suppress auto-disable via crafted prompt
│   └── MITIGATED: threshold operator-controlled; window=0 blocks emission
└── T1: malicious draft model alters token stream
    └── MITIGATED: AES-GCM auth tag; path allowlist; shared loader validation
```

### 12.8 Speculative Decoding Test Coverage

| Test | Threat | File |
|------|--------|------|
| `t2_rejected_suffix_never_emitted` | T2 | `tests/security_speculative_test.rs` |
| `t3_config_fields_contain_no_pii_types` | T3 | `tests/security_speculative_test.rs` |
| `t5_auto_disable_fires_below_threshold` | T5 | `tests/security_speculative_test.rs` |
| `t5_fallback_plan_has_zero_window` | T5 | `tests/security_speculative_test.rs` |
| `t4_disabled_config_yields_single_model_plan` | T4 | `tests/security_speculative_test.rs` |
