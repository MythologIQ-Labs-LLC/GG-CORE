# GG-CORE v1.0.0 capability and issue reconciliation (working evidence ledger)

**Date:** 2026-11-15. **Status:** Review-ready reconciliation, **not** owner-approved scope lock, release GO, or a replacement for source/packaged evidence.

**Purpose:** Separate the three overlapping v1 tracking responsibilities and reconcile obsolete issue wording with merged implementation. Preserve the owner's direction: next frozen candidate must implement and qualify **all owner-included standalone v1.0.0 capability commitments**. Do not auto-exclude anything, defer an included feature as "post-1.0", or treat source tests as installed-product acceptance.

## Single-responsibility control plane

| Issue | Sole authoritative question | Completion gate |
| --- | --- | --- |
| [#157](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/157) | Product destination and overall Wayfinder progress | Linked decisions reconciled; no implementation claim |
| [#158](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/158) | **Inventory:** what exists, at what maturity, with what evidence and gaps? | Every identified capability has implementation/test/packaged status, paths, risks, and related issue; no exclusions decided here |
| [#159](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/159) | **Standalone operator/interface decisions** | Owner answers open decisions; optional HTTP inference remains **unapproved**, IPC and offline-first remain default |
| [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) | **Commitment:** what exact features and model/platform/accelerator/API matrix must v1 ship? | Every candidate capability has owner IN/EXCLUDE/UNDECIDED disposition; no invented owner answer |
| [#161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161) | **Delivery and proof:** which ordered internal milestones and full integrated GO/NO-GO checks prove #160? | Exact-source, packaged real-model security, performance, clean-install, conformance and owner GO receipts; no partial public RC |
| [#149](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/149) | Adaptive-model sub-map | Child of standalone product objective, not alternative release target |

**Vocabulary:** merged = integrated into source; source-tested = tests pass on an identified head; packaged-tested = real produced binaries tested under declared defaults; installed-qualified = real standalone install and task work; release-qualified = all owner-approved cells and release controls accepted. A row marked `verified` in [FEATURE_INDEX](FEATURE_INDEX.md) means a named test binding exists, **not** that packaged/installed behavior is demonstrated.

## Reconciled current-state ledger

| Capability / issue | Confirmed implementation state | Still required before full v1 qualification |
| --- | --- | --- |
| Windows clean-host dependency packaging [#162](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/162) | **Issue closed; PR #166 merged.** Offline clean-guest evidence recorded; owner selected app-local Microsoft-signed VC++/OpenMP DLLs | Final exact-release package clean-host regression, license notices and GG-CORE binary signing/verification; do **not** repeat stale issue wording that #162 is still open |
| Windows IPC burst handling [#163](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/163) | **Issue closed; PR #165 merged.** Windows 8-client/burst and auth 401→handshake tests recorded | Re-run on exact v1 packaged/installed head at advertised concurrency and authentication limits; do not count the original defective RC1 baseline as proof |
| Format/capability fail-closed dispatch [#169](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/169) | **PR #170 merged; #169 remains open.** Code handles unsupported formats, capability mismatch, per-model manifests, Windows reparse-point checks | Verify current main behavior, packaged real GGUF/ONNX family matrix and negative paths; disposition #169 against its own full criteria, not merely PR merge |
| Typed ONNX IPC [#171](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/171) | **Open functional acceptance gap.** ONNX backend embed/classify and load plumbing are not proof of typed authenticated IPC serving | Versioned typed outputs, auth/admission/security limits, real licensed embedding/classifier models, backwards-compatible GGUF generation and installed tests |
| GGUF stop/quiescence [#184](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/184) | **#195 merged deterministic prefill fix; #200 draft opt-in stage trace.** Existing Windows/Qwen CPU proof demonstrates short *observed* tails, not hard native interruption | #200's report must return nonzero for failed acceptance, be retested on current main, independently reviewed; declare limits for other models, platforms, installed flows, packaged streaming/60-s deadlines |
| Model load/switch/reclaim [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137) | Mixed-model and cold-switch work reported on separate branch; **open, no installed v1 acceptance**. Existing prototype has reported compile/lint and readiness/journal review blockers | Authenticated A→B→A across families/ONNX, bound memory and true release, readiness, idempotent recovery, installed acceptance, no misleading ready claims |
| Resource configuration [#194](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/194) | Explicit memory caps are strict; remaining `GG_CORE_*` resource variables have potentially silent clamp/fallback | Per-variable authority policy, strict or reported clamp with parity between `config validate` and `serve`, targeted negative tests |
| History false positives [#186](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/186) | Reported from downstream; cause **not independently reproduced** | Minimal sanitized repro through real IPC, policy-safe source/role treatment; never weaken guard based on anecdote |
| Benchmarks [#156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156), [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136) | CPU baselines and N7-specific timing exist; no complete secured packaged performance matrix | Comparable real models, multi-client, CPU/GPU, TTFT/ITL/RSS/quality, security-overhead and regression thresholds |
| Capability-aware backend/hardware selection [#48–#50](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/49), [#154](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/154) | Various feature paths and experimental/simulated components exist | Actual CUDA/Metal/other owner-approved cells, backend dispatch, deterministic fallback, fit/quality/performance tests; no GPU-device detection = acceleration claim |
| Stable interfaces [#112–#114](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112) | Existing Rust/FFI/Python/IPC surfaces; pre-v1 compatibility not stable | Exact versioned API/ABI/protocol, conformance fixtures, distribution/rollback guarantees |
| K8s IPC observability [#164](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/164) | Health commands / probe templates exist; no installed K8s production proof | Owner-supported scope and actual air-gapped probe/metric/security test, no automatically authorized network listener |
| Backend research [#196](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/196), [#70](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/70), [#115](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/115), [#48](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/48), [#51](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/51) | Hologram, ONNX generation and BitNet alternatives are investigation subjects, **not operational GG-CORE capability claims** | Equal evidence/admissibility bar for GGUF/ONNX/Hologram; explicit owner inclusion/exclusion and model-support decisions; no preselected backend winner |

## Work sequence, without owner scope drift

1. **Current N7 lane:** complete draft #200 independently, without introducing native hard abort or interrupting ongoing VM qualification. Only promote after executable report gate, current-main integration and exact-identity acceptance.
2. **Reconcile merged source to the capability inventory:** close stale factual claims on #158 (notably #162/#163 still-open text), verify #169 merged implementation against actual acceptance; retain #171 and #137 gaps, avoiding duplicate code.
3. **Lock outstanding standalone decisions:** #159 interface Q2, approved model/OS/accelerator/task cells and research dispositions in #160. Record owner decisions; do not infer them.
4. **Execute dependency-ordered internal slices:** format/capability contract → typed ONNX serving → secure multi-model lifecycle → validated resource/config semantics → qualified GPU and advanced paths → standalone distribution, stable API and perf/security acceptance.
5. **Integrated release gate #161:** complete all #160-included features and test on the real supported matrix before any feature-complete RC or stable GO.

## Non-negotiable release boundaries

- GG-CORE is upstream, standalone and has no CoreForge runtime dependency. CoreForge owns downstream UI, installer, policy and pins.
- Preserve immutable published `v0.10.0-rc.1`, its provenance and downstream fallback; no release/tag/pin changes from this reconciliation.
- Authenticated local IPC, zero implicit daemon network/model-download authority and fail-closed admission remain defaults. HTTP inference is a pending owner decision, not approved code.
- Source and feature-index test binding **cannot** substitute for qualified packaged standalone execution, clean install, named-pipe security, model diversity, memory reclamation, real GPU, benchmark security parity or independent release approval.
- This ledger is a **snapshot**. Before using any row as authority for release promotion, recheck live issue decisions, commit identities and exact test records.
