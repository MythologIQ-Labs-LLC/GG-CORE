# GG-CORE Architecture Index

**Status:** Canonical entry point for architecture navigation. This index records where design authority and evidence reside; it is not a new specification or a claim of release qualification.

GG-CORE is a **standalone, upstream local inference runtime**. CoreForge is one downstream application consumer. GG-CORE's build, security boundary, inference backends, release qualification, and architecture are owned in this repository, not by CoreForge.

## Authority and precedence

1. **Implementation and executable tests:** `core-runtime/` and its tests determine actual behavior. A design document never overrides verified source behavior.
2. **Security and interface contracts:** [SECURITY.md](SECURITY.md), [architecture plan](docs/ARCHITECTURE_PLAN.md), and the [runtime architecture](docs/architecture/CORE_RUNTIME_ARCHITECTURE.md) describe intended boundaries and supported integration paths. Treat contradictions with source as documentation defects.
3. **Release and capability evidence:** [feature index](docs/FEATURE_INDEX.md), [README verification](docs/README_VERIFICATION.md), and [release support](docs/RELEASE_SUPPORT_AND_MAINTENANCE.md) determine what has been demonstrated for specific versions. An implemented or feature-gated backend is not automatically an installed release capability.
4. **Decisions and historical records:** ADRs and integration plans provide context, not blanket current acceptance. Check exact commits and linked issues when interpreting old plans or audit reports.

## Architecture and decision map

| Concern | Canonical starting point | Boundary |
| --- | --- | --- |
| Runtime design and module ownership | [Core Runtime Architecture](docs/architecture/CORE_RUNTIME_ARCHITECTURE.md) | Standalone upstream compute and security pipeline |
| Structural design and interface planning | [Architecture Plan](docs/ARCHITECTURE_PLAN.md) | Intended interfaces; verify against code |
| Public integration and supported features | [README](README.md), [Feature Index](docs/FEATURE_INDEX.md) | Per-feature and per-surface evidence, not assumed maturity |
| Model/backend selection authority | [Backend Capability Contract](docs/integration/V1_BACKEND_CAPABILITY_CONTRACT.md), [selection source](core-runtime/src/models/model_selection_policy.rs) | Per-model advisory decisions only; no unauthorized runtime switching or qualified-performance claims |
| Conservative model lifecycle and cold-switch design | [standalone switching acceptance](docs/integration/V1_STANDALONE_SWITCH_ACCEPTANCE.md), [IPC contract](docs/IPC_PROTOCOL_SCHEMA.md), [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137) | Authenticated development implementation; never call registered models serving-ready without a real task probe |
| Proposed v1.1 adaptive heterogeneous inference | [Architecture proposal](docs/architecture/V1_1_ADAPTIVE_HETEROGENEOUS_INFERENCE.md) | CPU/GPU plan selection and shared-resource governance; design only, no change to v1.0.0 scope or gates |
| Embedded API and IPC V1 compatibility | [v1 contract baseline](docs/integration/V1_EMBEDDED_CONTRACT_BASELINE.md), [IPC schema](docs/IPC_PROTOCOL_SCHEMA.md), [CLI status source](core-runtime/src/cli/status.rs) | Pre-freeze source/wire evidence; unmeasured status quantiles are under review in [#268](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/268), not a published ABI or benchmark receipt |
| Unsafe Rust and stream-worker lifetime | [Historical unsafe audit](docs/security/UNSAFE_AUDIT.md), [P0 defect #219](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/219) | The historical zero-high-risk conclusion is superseded; native task ownership and admission/cancellation acceptance remain release holds |
| Security and platform containment | [Security Policy](SECURITY.md) | Windows Job Objects do not isolate network or filesystem; platform guarantees vary |
| Internal governance registry | [Governance Index](docs/GOVERNANCE_INDEX.md) | Process artifact map, separate from architecture ownership |
| Speculative decoding | [ADR-007 TierSynergy](docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md) | Experimental/dormant paths must not be called production-qualified |
| RC support and rollback | [Release Support and Maintenance](docs/RELEASE_SUPPORT_AND_MAINTENANCE.md) | Pin exact source/binary identities and tested scope |
| Downstream consumer contract | [CoreForge Producer Contract](docs/integration/COREFORGE_PRODUCER_CONTRACT.md) | Consumer integration contract; not GG-CORE's primary architecture |
| Documentation claim verification | [README Verification](docs/README_VERIFICATION.md) | Evidence applies to recorded source and date, not automatically to a newer `main` |

## Architecture change discipline

- Maintain this root index when an architecture-level component, authority boundary, decision record, or canonical contract is added, removed, or superseded.
- Update the specific owning document and its tests/evidence in the same change when behavior or maturity shifts. Do not duplicate entire specifications here.
- Distinguish **implemented**, **tested**, **packaged**, **installed**, and **released**. Do not upgrade a claim simply because a PR merged.
- Preserve historical records with their dates and exact tested identities; clearly mark superseded plans instead of retroactively presenting them as current.
- GG-CORE must not depend on CoreForge for its own functionality. Any CoreForge-specific lifecycle, UI, or installer design belongs downstream; upstream IPC and library contracts belong here.
