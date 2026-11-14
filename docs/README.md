# GG-CORE documentation

GG-CORE is a **standalone, offline-first inference runtime**. Documentation must distinguish the **published `v0.10.0-rc.1` prerelease**, protected `main`, and **unmerged v1 development branches**. A compiled module is not a packaged, installed or hardware-qualified capability.

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
