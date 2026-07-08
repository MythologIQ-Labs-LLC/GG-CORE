# Governance Index

**Last Reviewed**: 2026-07-08

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
