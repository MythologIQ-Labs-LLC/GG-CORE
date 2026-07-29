# Governance Index

**Last Reviewed**: 2026-07-28

A single authoritative map of every governance artifact in this project, organized
into six freshness tiers with explicit drift contracts. A stale entry here is
itself a Tier 1 drift bug, so the index is self-policing. See
`qor/references/doctrine-governance-index.md` for the model and contracts.

## Tier 1 — Canonical Source

MUST be current at every cycle close. Drift signal: wrong version / wrong state / missing recent entries.

| Artifact | Path | Freshness marker |
|----------|------|------------------|
| Meta Ledger | `docs/META_LEDGER.md` | latest sealed entry |
| System State | `docs/SYSTEM_STATE.md` | latest phase snapshot |
| Concept | `docs/CONCEPT.md` | stable |
| Architecture Plan | `docs/ARCHITECTURE_PLAN.md` | stable |
| Backlog | `docs/BACKLOG.md` | open items current |
| Feature Index | `docs/FEATURE_INDEX.md` | every feature has a test |
| Changelog | `CHANGELOG.md` | latest release stamped |
| README | `README.md` | badges current |

## Tier 2 — Doctrine & Policy

Stable; changes are explicit doctrine events. Drift signal: rules contradict each other or operator memory.

| Artifact | Path |
|----------|------|
| Project governance instructions | `CLAUDE.md` |
| Security policy | `SECURITY.md` |
| Contributor license agreement | `CLA.md` |
| Shadow Genome (failure doctrine) | `docs/SHADOW_GENOME.md` |

## Tier 3 — Active Initiative

Live until close; ages out at substantiate. Drift signal: shipped feature still tracked as pending.

| Artifact | Path | Opened |
|----------|------|--------|
| Governance remediation (artifact reconstruction) | `.qor/session/` | 2026-07-08 |

## Tier 4 — Per-Plan Artifact

Live for plan duration; archived at substantiate. Drift signal: plan shipped but artifact still presents as open.

| Artifact | Path | Plan |
|----------|------|------|
| Cycle-1 hardening plan | `docs/plan-runtime-hardening-cycle1-2026-07-08.md` | runtime-hardening-cycle1 |
| Cycle-1 research brief | `docs/research-brief-runtime-optimization-hardening-2026-07-08.md` | runtime-hardening-cycle1 |
| Cycle-2 hardening plan (rev.3) | `docs/plan-runtime-hardening-cycle2-2026-07-08.md` | runtime-hardening-cycle2 |
| B-20 KV isolation redesign spec | `docs/plan-b20-kv-isolation-redesign.md` | b20-kv-isolation |
| ADR-007 epic execution plan | `docs/plan-adr007-epic-execution.md` | adr007-epic |
| Merge/integration runbook | `docs/runbook-merge-integration-sequence.md` | integration-sequence |
| Secure inference facade plan | `docs/plan-secure-inference-facade-2026-07-25.md` | secure-inference-facade |
| Security chain wiring plan | `docs/plan-security-chain-wiring-2026-07-25.md` | security-chain-wiring |
| ONNX classifier plan | `docs/plan-onnx-classifier-2026-07-26.md` | onnx-classifier |
| pyo3 migration plan | `docs/plan-pyo3-migration-2026-07-26.md` | pyo3-migration |
| B-25 CI foundation plan | `docs/plan-b25-ci-foundation-2026-07-26.md` | b25-ci-foundation |
| B-25b FFI/Python reroute plan | `docs/plan-b25b-ffi-python-reroute-2026-07-26.md` | b25b-ffi-python-reroute |
| rand 0.9 migration plan | `docs/plan-rand-0.9-migration-2026-07-27.md` | rand-0.9-migration |
| B-24a stream terminal plan | `docs/plan-b24a-stream-terminal-2026-07-27.md` | b24a-stream-terminal |
| B-24b streaming egress plan | `docs/plan-b24b-streaming-egress-2026-07-27.md` | b24b-streaming-egress |
| B-28 tokenizer plan | `docs/plan-b28-tokenizer-2026-07-27.md` | b28-tokenizer |
| B-29a ONNX dispatch plan | `docs/plan-b29a-onnx-dispatch-2026-07-28.md` | b29a-onnx-dispatch |
| mistral.rs perf research brief | `docs/research-brief-mistral-rs-rust-inference-perf-2026-07-25.md` | mistral-rs-perf |
| Open-issues compat research brief | `docs/research-brief-open-issues-compat-support-perf-2026-07-25.md` | open-issues-compat |
| Presidio PII comparison brief | `docs/research-brief-presidio-pii-comparison-2026-07-25.md` | presidio-pii |
| B-24 streaming egress brief | `docs/research-brief-b24-streaming-egress-2026-07-27.md` | b24-streaming-egress |
| B-24b streaming egress brief | `docs/research-brief-b24b-streaming-egress-2026-07-27.md` | b24b-streaming-egress |
| B-25 CI legs research brief | `docs/research-brief-b25-ci-legs-ffi-python-2026-07-26.md` | b25-ci-legs |
| B-28 tokenizer research brief | `docs/research-brief-b28-tokenizer-2026-07-27.md` | b28-tokenizer |
| Backlog reconciliation brief | `docs/research-brief-backlog-reconciliation-2026-07-27.md` | backlog-reconciliation |
| rand 0.9 migration brief | `docs/research-brief-rand-0.9-migration-2026-07-27.md` | rand-0.9-migration |
| B-29 ONNX dispatch research brief | `docs/research-brief-b29-onnx-dispatch-2026-07-28.md` | b29-onnx-dispatch |
| B-29b-1 Model trait unification plan | `docs/plan-b29b1-model-trait-unification-2026-07-28.md` | b29b1-model-trait-unification |
| B-29b registry unification research brief | `docs/research-brief-b29b-registry-unification-2026-07-28.md` | b29b-registry-unification |
| B-29b-2 manifest dispatch plan | `docs/plan-b29b2-manifest-dispatch-2026-07-28.md` | b29b2-manifest-dispatch |
| B-29b-2 manifest dispatch research brief | `docs/research-brief-b29b2-manifest-dispatch-2026-07-28.md` | b29b2-manifest-dispatch |
| B-07 degraded-mode plan | `docs/plan-b07-degraded-mode-2026-07-28.md` | b07-degraded-mode |
| B-07 degraded-mode research brief | `docs/research-brief-b07-degraded-mode-2026-07-28.md` | b07-degraded-mode |
| B-16 sandbox Razor refactor plan | `docs/plan-b16-sandbox-unix-razor-2026-07-28.md` | b16-sandbox-unix-razor |
| B-16 sandbox Razor refactor research brief | `docs/research-brief-b16-sandbox-unix-razor-2026-07-28.md` | b16-sandbox-unix-razor |

## Tier 5 — Reference Material

Informational, slow-drift. Drift signal: factual claims diverge from current code.

| Artifact | Path |
|----------|------|
| Roadmap | `ROADMAP.md` |
| IPC protocol schema | `docs/IPC_PROTOCOL_SCHEMA.md` |
| Usage guide | `docs/USAGE_GUIDE.md` |
| Dependency analysis | `docs/DEPENDENCY_ANALYSIS.md` |
| Honest assessment | `docs/HONEST_ASSESSMENT.md` |
| Rust enterprise analysis | `docs/RUST_ENTERPRISE_ANALYSIS.md` |
| Tandem experiments proposal | `docs/TANDEM_EXPERIMENTS_PROPOSAL.md` |
| Recommended models | `docs/RECOMMENDED_MODELS.md` |
| Benchmarks | `docs/BENCHMARKS.md` |
| Architecture decision records | `docs/architecture/` |
| Security docs | `docs/security/` |
| Analysis & reviews | `docs/analysis/`, `docs/review/` |

## Tier 6 — Archived

Frozen historical record. Drift signal: none (frozen).

| Archive | Path |
|---------|------|
| _none yet_ | `docs/archive/` (create on first retirement) |

## How to add a governance artifact

1. Create the file in the same commit that registers it here.
2. Add a row to the tier whose freshness contract matches the file's lifecycle.
3. Refresh **Last Reviewed** above.

## How to retire a governance artifact

1. Move the file to the Tier 6 archive path.
2. Move its row from its live tier to Tier 6 (or delete it if superseded).
3. Refresh **Last Reviewed** above.
