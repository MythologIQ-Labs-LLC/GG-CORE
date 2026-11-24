# Exact-revision Rust consumption: pre-v1 distribution contract

**Status:** Source-level distribution by pinned Cargo Git revision (#113).
Not a published crates.io package, stable v1 API promise, signed build,
hardware qualification or release GO. Inference remains offline; deliberate
dependency fetching at build time is a separate operation.

## Exact pin, not a floating version

~~~toml
[dependencies]
gg-core = { git = "https://github.com/MythologIQ-Labs-LLC/GG-CORE", rev = "<FULL_40_HEX_GG_CORE_COMMIT>", version = "=0.10.0-rc.1", default-features = false }
~~~

Replace the full SHA with a reviewed producer source revision. Commit
the consumer Cargo.lock; always build with --locked. The 0.10.0-rc.1
version string is intentionally not sufficient to distinguish pre-v1
development commits. Features such as GGUF and ONNX must be explicitly
opted in and separately qualified on supported hosts.

## Embedded identity

External Rust consumers can read the additive pre-v1 API
gg_core::build_identity::{VERSION, COMMIT, BUILD_DATE}.
The CLI version --json reports the same source identity.

When used as a Cargo Git dependency, GG-CORE builds from its own Git
checkout and must ignore the consuming application's unrelated GITHUB_SHA.
A source archive with no Git metadata must provide explicit full-hex
GG_CORE_BUILD_COMMIT or report "unknown", which is not an acceptable
release identity. This override is not a cryptographic authenticity proof.

## Upgrade and rollback

1. Review and record the new producer Git SHA, backend features, security
   baseline, supported platform and previous working SHA.
2. Change both dependency rev and Cargo.lock. Verify the consumer's actual
   API, IPC error semantics, model admission and native packaged acceptance.
3. To roll back, restore both dependency declaration and Cargo.lock, rebuild
   and repeat the same source and native validation. Model/data migrations
   and released daemon binaries need independent rollback receipts.
4. Stable embedded API/ABI guarantees are governed by #112. Present
   pre-v1 revisions may require explicitly migrated callers.

## Independent consumer qualification

core-runtime/scripts/git_consumer_contract.py creates a clean external
Cargo crate with a full-SHA local file:// Git dependency on this repository.
It verifies that both the generated Cargo.lock and the resolved package
metadata carry exactly that version and source Git SHA. It intentionally
injects an unrelated GITHUB_SHA. On Linux, the external crate additionally
compiles and executes a public GG-CORE library entry and verifies the
compiled producer identity. Windows/macOS verify exact source resolution
and additionally run the existing full product compilation/platform suite.

This is source-distribution evidence, **not** a signed portable installer,
model-serving proof, published package registry, or v1 compatibility GO.
The remaining installed/ABI gates are tracked in #112, #114, #171 and #161.
