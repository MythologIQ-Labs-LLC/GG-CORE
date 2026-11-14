# GG-CORE documentation

GG-CORE is a **standalone, offline-first inference runtime**. Documentation must distinguish the **published `v0.10.0-rc.1` prerelease**, protected `main`, and **unmerged v1 development branches**. A compiled module is not a packaged, installed or hardware-qualified capability.

## Canonical architecture owner register

This page is the **single canonical architecture-owner register** for GG-CORE
and the target of the mandatory [repository-root architecture
index](../ARCHITECTURE_INDEX.md). The root index routes here; it does not
independently accept architectural changes or duplicate their authority.
Use the claim owner below for current decisions, and keep the registration
current whenever an accepted decision is superseded.

| Claim/domain | Canonical owner (relative to this register) | Status and precedence |
| --- | --- | --- |
| Independent runtime identity and one-way dependency | [Standalone dependency direction](architecture/STANDALONE_DEPENDENCY_DIRECTION.md) | Owner-directed 2026-11-14; GG-CORE upstream, CoreForge primary local inference consumer, not a GG-CORE dependency |
| Current runtime architecture, compute and process boundaries | [Runtime architecture](architecture/CORE_RUNTIME_ARCHITECTURE.md) | Living source-grounded architecture; code and tested artifacts control behavior |
| Authenticated local IPC schema and compatibility | [IPC protocol schema](IPC_PROTOCOL_SCHEMA.md) | Protocol definition; version and published-artifact support must be verified |
| Security posture and supported OS containment | [SECURITY.md](../SECURITY.md) | Security policy and evidence, not universal network/filesystem isolation |
| Model/backend capability and verification mappings | [Feature index](FEATURE_INDEX.md) | Source-to-test evidence; unqualified release features remain excluded |
| Standalone build, CI and release boundaries | [Workflow and release boundaries](architecture/WORKFLOW_AND_RELEASE_BOUNDARIES.md) | Source-grounded workflow contract; v1 signing and publication gates remain separate |
| Producer release support and rollback | [Release support and maintenance](RELEASE_SUPPORT_AND_MAINTENANCE.md) | Released `v0.10.0-rc.1` distinct from protected `main` and v1 development |
| Current intended v1 scope and outstanding decisions | [Roadmap](../ROADMAP.md) and [v1 release owner gate #161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161) | Roadmap proposal and owner decisions; not shipping evidence |
| CoreForge-specific adoption and integration | [CoreForge producer contract](integration/COREFORGE_PRODUCER_CONTRACT.md) | Downstream consumer supplement; does not control upstream product design |
| Original scope and architecture intent | [Architecture plan](ARCHITECTURE_PLAN.md) and [Concept](CONCEPT.md) | Historical/aspirational references; living architecture controls current assertions |

### Architecture-document inventory and maturity

The following identifies **every current `docs/architecture/*.md` file** and
its owner role. Do not equate an old ADR's "Accepted" front matter with
requalification against the current source or installed release.

| Architecture document | Registered role |
| --- | --- |
| [ADR-006 Deployment Strategies](architecture/ADR-006-DEPLOYMENT-STRATEGIES.md) | Historical accepted design, not current Kubernetes/production deployment proof |
| [ADR-007 Consolidation Audit](architecture/ADR-007-CONSOLIDATION-AUDIT.md) | Dated audit and evidence; not a fresh architecture acceptance |
| [ADR-007 TierSynergy](architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md) | Historical accepted adaptive design; dormant or feature-gated paths need current tests |
| [Runtime Architecture](architecture/CORE_RUNTIME_ARCHITECTURE.md) | Living runtime architecture owner |
| [Scalability Remediation](architecture/SCALABILITY_REMEDIATION_UPGRADE_PATH.md) | Planned remediation, not delivered capability |
| [Standalone Dependency Direction](architecture/STANDALONE_DEPENDENCY_DIRECTION.md) | Current owner directive for upstream producer and downstream consumers |
| [v0.6.0 Trade-offs](architecture/V0.6.0_TRADE_OFFS.md) | Historical design decisions and context; no current certification implied |
| [Workflow and Release Boundaries](architecture/WORKFLOW_AND_RELEASE_BOUNDARIES.md) | Current release/CI description and explicitly open gates |

When two documents disagree, record drift and obtain an authorized disposition.
Preserve old decisions for provenance; never resolve conflicts by deleting
an inconvenient source. Structural or link-check PASS does not establish
semantic agreement, independent review, signed release, or installed acceptance.

## Start here

| Resource | What it covers |
| --- | --- |
| [Root README](../README.md) | Standalone build, serve, authenticated IPC, Rust/C/Python consumers |
| [Living runtime architecture](architecture/CORE_RUNTIME_ARCHITECTURE.md) | Security boundary, daemon and embedded paths, model-loading versus inference diagrams |
| [Standalone dependency direction](architecture/STANDALONE_DEPENDENCY_DIRECTION.md) | GG-CORE producer, CoreForge and others downstream; acceptance ownership |
| [Architecture plan](ARCHITECTURE_PLAN.md) | Historical file tree, interface and governance plan; current source and living architecture win on drift |
| [CI and release workflow boundaries](architecture/WORKFLOW_AND_RELEASE_BOUNDARIES.md) | Existing workflow triggers, packaged real-model acceptance, and the unimplemented v1 Windows Authenticode gate |
| [IPC schema](IPC_PROTOCOL_SCHEMA.md) | Length-prefixed authenticated local protocol; follow exact source and version compatibility |
| [Release support and maintenance](RELEASE_SUPPORT_AND_MAINTENANCE.md) | Verified published RC1, installed-platform caveats, hash checks, rollback |
| [Feature index](FEATURE_INDEX.md) | Source-to-direct-test mapping, **not** a blanket shipped-product compatibility claim |
| [README verification](README_VERIFICATION.md) | Claims versus evidence, including historical records clearly marked superseded |
| [Roadmap](../ROADMAP.md) | Standalone v1 targets and work-state distinctions |
| [Security policy](../SECURITY.md) | Vulnerability reporting and supported security boundaries |
| [Threat model](security/THREAT_MODEL.md) | Trust boundary and attack-surface analysis |
| [Benchmarks](BENCHMARKS.md) | Historical results and exact measurement limitations |

## Current work, not in the published RC

- [#137 model switching](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137): source-qualified in-process Windows cold switching; authenticated IPC/status extension remains **not native-qualified** and must prove task-level readiness, multi-client behavior and repeated-cycle resource stability.
- [#49 capabilities](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/49): source-qualified authenticated read-only capability snapshot; zero release-qualified cells until pinned installed-model evidence.
- [#161 standalone v1 release](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161): integrates owner-approved scope, platform/model acceptance, and authentic Windows GG-CORE binary signing. No CoreForge-specific product gate.

The original analysis, security, build, testing and historical audit records remain in their respective directories. They are research or dated evidence, not substitutes for the live source, exact SHA CI, published assets, or installed-product qualification.
