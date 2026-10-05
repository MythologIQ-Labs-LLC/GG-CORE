# System State

**Status**: This document was a point-in-time snapshot (last generated
2026-07-08) and had drifted far from reality — it described branches,
file trees, counts, and open issues that no longer exist. It was retired
during the issue #107 documentation reconciliation (2026-10-05) rather
than silently left wrong.

## Where live state actually lives

| Question | Authoritative source |
| --- | --- |
| What is shipped, at what maturity? | `ROADMAP.md` (rebuilt 2026-10-05), `docs/FEATURE_INDEX.md` (verified rows with test citations) |
| What changed, when? | `CHANGELOG.md`; git history; `docs/META_LEDGER.md` (Merkle-chained decision log) |
| What work is open? | GitHub issues; `docs/BACKLOG.md` (pointer layer) |
| Security posture | `SECURITY.md` (honest maturity table incl. library-only items) |
| Architecture | `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` + ADRs |
| CI state | `.github/workflows/rust.yml` (lint/test/features/bench/supply-chain), `release.yml` |

If a future governance cycle needs a frozen snapshot again, generate it
from the sources above at seal time and date it; do not hand-maintain it.
