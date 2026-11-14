# Architecture index

**Role:** Mandatory repository-root architecture entrypoint for GG-CORE. This
file is a routing and drift-control document, **not** a second source of
architectural decisions, an approval record, or proof that a feature shipped.

The **single canonical architectural owner register** for this repository is
[GG-CORE's documentation and architecture owner register](docs/README.md).
Start there before architecture-affecting planning, implementation, review,
release qualification, or architectural supersession. Follow that register to
the exact owner of each claim. Source and executed evidence establish behavior,
but neither silently supersedes an accepted design decision.

## Binding product and dependency direction

**GG-CORE is an independent, upstream, standalone local inference runtime.**
**CoreForge is a downstream consumer that uses GG-CORE as its primary local
inference engine**, packaged as a local inference sidecar. The CoreForge UI may
make this capability appear integral to the application; that does not change
the producer/consumer boundary or give CoreForge authority over GG-CORE's
versioning, models, runtime architecture, release acceptance, or public APIs.

Build-time and runtime dependency direction: **CoreForge depends on GG-CORE,
never GG-CORE on CoreForge.** GG-CORE must be usable and independently
qualified without CoreForge. Other compatible hosts may consume the same
qualified versioned interfaces, subject to actual platform, protocol, backend,
model and security support. CoreForge-specific personas, tools, external
providers, UX and installed acceptance belong to the downstream application.

The owning document for this direction is
[standalone dependency direction](docs/architecture/STANDALONE_DEPENDENCY_DIRECTION.md).
The [standalone runtime architecture](docs/architecture/CORE_RUNTIME_ARCHITECTURE.md)
owns GG-CORE internal design. The
[CoreForge producer contract](docs/integration/COREFORGE_PRODUCER_CONTRACT.md)
is a consumer-specific integration reference, **not** GG-CORE's architecture
authority. No automatic HTTP/network inference service, universal OS sandbox
or unverified model/backend support is implied.

## Governance rules

1. **Identify the owner:** Read the single register at
   [docs/README.md](docs/README.md), then the accepted contract or ADR and
   its exact revision. Do not promote proposals, historical records or
   aspirational plans into implemented or accepted claims.
2. **Detect drift:** When code, diagrams, tests, a release artifact and the
   accepted architecture disagree, record the conflicting claims, exact
   revisions, owner and effects. Do not silently edit the easiest document to
   manufacture agreement.
3. **Change under real authority:** Proposals identify old/new claim owners,
   the replacement design, interfaces, dependencies, security/authority
   effects, migrations, failure modes and regression requirements. Only the
   designated owner can approve an architectural supersession.
4. **Update together:** A governed change updates the owning document and
   its register entry in the same reviewed change. Preserve historical
   provenance, link superseded claims, and requalify invalidated evidence.
5. **Verify:** Check every listed local link and unique owner, check
   architectural documentation/test gates, and run
   `qortara check architecture-index` **when Qortara Logic is installed and
   configured for this checkout**. A structural PASS is not approval,
   independent review, release GO, or proof of installed behavior.
6. **Respect release independence:** GG-CORE's published
   `v0.10.0-rc.1`, protected main, and unreleased v1 work remain distinct.
   No consumer adoption or CoreForge installer test substitutes for the
   standalone GG-CORE release gates. Do not rewrite a frozen artifact.

**Status:** Owner-directed documentation/governance change under review.
The existence of this entrypoint does not by itself establish that any
architecture supersession is accepted or that Qortara checks ran.
