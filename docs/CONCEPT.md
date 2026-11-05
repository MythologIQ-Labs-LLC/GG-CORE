# Project Concept

> **Design vision, not a verified release-specification.** This early concept includes aspirational sandbox, complete network-denial and performance statements that do **not** describe every platform in the current `0.10.0-rc.1` candidate. Current verified capabilities and risks are in [SECURITY.md](../SECURITY.md), the [producer contract](integration/COREFORGE_PRODUCER_CONTRACT.md), and [release gate #130](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/130). Do not treat this document's historical goals or illustrative numbers as achieved benchmark or certification evidence.

## Why (One Sentence)

**GG-CORE** (Greatest Good - Contained Offline Restricted Execution) is a sandboxed, offline inference engine that performs model execution only and has no authority over data, tools, or system actions.

## Philosophy: Greatest Good

Built on triage principles ("Greatest Good for the Greatest Number"), GG-CORE prioritizes system stability and fair resource allocation over individual request optimization. In a multi-tenant AI world, every inference request competes for limited compute—GG-CORE ensures no single tenant can monopolize resources or destabilize the system.

## C.O.R.E. Principles

| Principle | Meaning |
|-----------|---------|
| **Contained** | Sandbox with no ambient privileges |
| **Offline** | Zero network access (inbound/outbound blocked) |
| **Restricted** | IPC-only communication with authenticated callers |
| **Execution** | Pure compute, no business logic or decision authority |

## Vibe (Three Keywords)

1. **Minimal** - Pure function: tokens in, tokens out. No scope creep.
2. **Secure** - Sandboxed, isolated, zero network access, least privilege.
3. **Traceable** - Deterministic execution with observable metrics, no hidden state.

## Anti-Goals (What This Is NOT)

- NOT an agent (no autonomous decision-making)
- NOT a plugin host (no external code execution)
- NOT a connector layer (no network access)
- NOT a policy engine (no governance authority)
- NOT a memory store (no persistent state)
- NOT an orchestrator (no workflow management)

## Design Pillars Alignment

| Pillar | How CORE Supports It |
|--------|---------------------|
| Data Sovereignty | No persistent storage or network - cannot leak data |
| System Integrity | Sandbox + isolation - cannot bypass boundaries |
| AI Governance | No authority - cannot act independently |
| System Reliability | Stateless + restartable - safe recovery |

---

## Performance Goals

### Measurable Objectives

| Goal | Metric | Validation Method |
|------|--------|-------------------|
| **Secure** | 50+ security tests passing | TDD-Light security validation suite |
| **Compute Efficient** | CPU-only, no GPU required | Benchmark on 4-8 core reference hardware |
| **Fast Inference** | Meet tier targets (see below) | Criterion benchmark suite |
| **Memory Efficient** | RSS ≤ 1.5x model file size | Memory overhead benchmarks |

### Tier Progression Targets

The original 40–88% "security tax" was **an unverified planning assumption**, not a measured cost. A fair, secured-path benchmark remains open under [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136).

#### Tier 1: Minimum Viable (Baseline)

| Metric | Target | Rationale |
|--------|--------|-----------|
| Generation | >10 tok/s | Safe floor for Phi-3 Q4 on 4-core CPU |
| Classification P95 | <100ms | Generous for sandboxed ONNX |
| Memory Ratio | <1.5x | Industry standard ceiling |

**Intended test:** Once measured on a qualified host, the target would help evaluate sandbox overhead; the target is **not** evidence that it has been met.

#### Tier 2: Competitive (Stretch)

| Metric | Target | vs Industry |
|--------|--------|-------------|
| Generation | >25 tok/s | ~30% of CPU Ollama (80 tok/s) |
| Classification P95 | <20ms | ~10x slower than bare ONNX (1.86ms) |
| Memory Ratio | <1.35x | Match llama.cpp baseline |

**Requires**: IPC optimization, memory-mapped loading, thread tuning, KV-cache optimization.

#### Tier 3: Optimized (Future)

| Metric | Target | vs Industry |
|--------|--------|-------------|
| Generation | >50 tok/s | ~60% of CPU Ollama |
| Classification P95 | <5ms | ~3x slower than bare ONNX |
| Memory Ratio | <1.25x | Aggressive memory pooling |

**Requires**: Custom allocator, SIMD tokenization, speculative decoding.

### External Reference Benchmarks

| Runtime | Hardware | Performance | Source |
|---------|----------|-------------|--------|
| llama.cpp | M2 Ultra | 109 tok/s (TG) | GitHub #4167 |
| Ollama | 16-core EPYC (CPU) | 80 tok/s | Medium benchmarks |
| vLLM | 16-core EPYC (CPU) | 55 tok/s | Medium benchmarks |
| ONNX Runtime | Azure Cobalt 100 | 1.86ms / 538 inf/s | Arm Learn |

### Unverified performance target illustration (not benchmark data)

```
Unsandboxed Ollama CPU:  ████████████████████ 80 tok/s (100%)
Tier 3 Target:           ████████████         50 tok/s (62%)
Tier 2 Target:           ██████               25 tok/s (31%)
Tier 1 Target:           ███                  10 tok/s (12%)
```

**No verified 40–88% slowdown has been established.** These were possible sources of overhead, not measured deductions: platform-dependent process controls, IPC serialization and security scanning. Linux seccomp, Windows Job Objects and macOS have different isolation properties. The current RC does not claim an AppContainer confinement boundary.

---
