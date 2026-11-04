# GG-CORE: First Release Candidate Acceptance

**Status: OPEN, not release-qualified.** Owner release gate: [#130](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/130).

**Target:** `v0.10.0-rc.1` (recommended; version not yet changed or tagged). The Rust package is currently `0.9.0`. A pre-release is not a claim that the public 1.0 API is stable.

## What changed in the release workflow

`.github/workflows/release.yml` now supports two paths:

- **Manual `workflow_dispatch`:** runs verification, builds GGUF-enabled CLI + C FFI for Linux, Windows, and macOS, validates packaged files and CLI version, and uploads a checksum-verified `gg-core-release-bundle` artifact. **It does not create a GitHub Release.** This is the release-package dress rehearsal.
- **Version tag `vX.Y.Z[-rc.N]`:** runs the same checks and packages, requires `Cargo.toml` to match the tag and a real matching `CHANGELOG.md` section, then publishes the assembled release with SHA256SUMS. The job refuses empty stub release notes.

All advertised standalone inference packages must actually compile the `gguf` feature. A successful `ffi`-only build is not sufficient. If any supported OS cannot build or execute a backend-enabled package, repair the platform build or explicitly narrow the supported RC matrix **before tagging**.

## Execution and evidence

1. Merge the workflow PR only after review and successful existing Rust/CodeQL CI. On `main`, open GitHub Actions → Release → Run workflow. Choose the exact release source branch/commit. Manual dispatch must leave **no** GitHub Release or tag.
2. Confirm the `verify`, all three `build` jobs, and `assemble` succeeded. Download `gg-core-release-bundle`. Inspect the three archives and run `sha256sum -c SHA256SUMS` in the extracted bundle folder.
3. On clean Linux x86_64, Windows x86_64, and the built macOS architecture, unpack the respective archive. Run the binary's `version` and `--help`. Inspect the shared library and `gg_core.h`. Verify native loader dependencies and document toolchain/runtime prerequisites.
4. Use an approved real GGUF fixture with a known SHA256 and manifest. Run `serve --model` with a nonempty `CORE_AUTH_TOKEN` and the **platform default sandbox**. Verify readiness, listing, non-streaming and streaming inference, authenticated model load/unload, failed-load cleanup, cancellation where supported, bounded resource use, and graceful shutdown. Capture exact commands, logs, model provenance/license, hashes and results without publishing model bytes or secrets.
5. **Linux sandbox gate:** prove actual token generation while default seccomp is applied, not merely successful IPC probes. Reject a `GG_CORE_SANDBOX=off` workaround as a passing sandbox test. Check for unexpected SIGSYS termination.
6. **Security gate:** privately assess filesystem-indirection/path confinement and malformed-present manifest handling; verify no unauthorized model read or silent integrity downgrade. Check default authentication boundary and file/socket permissions. Confirm Windows Job Object restrictions on a normal user account. Document that macOS currently has no enforced daemon sandbox by default.
7. **Owner gate:** enable and verify GitHub private vulnerability reporting, the channel documented in `SECURITY.md`. Maintain a dated, exact-SHA acceptance record including all platform results, residual risks, security exceptions, rollback, and explicit owner approval.
8. Only after the above is accepted, bump the source to the selected prerelease version (recommend `0.10.0-rc.1`) in `core-runtime/Cargo.toml` and `Cargo.lock`, create actual `CHANGELOG.md` notes, and correct README/SECURITY support labels. Re-run the manual dress rehearsal on **that exact source commit** before pushing the tag. Pushing the tag is an owner action.

**No substitute evidence:** A green default-feature Rust build, YAML inspection, a backend-less CLI `version` response, green CodeQL, or checksum validity alone does not prove a working inference release. A successful release workflow also does not prove every security claim.

## Sign-off record (fill from actual runs)

| Gate | Receipt | Result |
| --- | --- | --- |
| Exact candidate source SHA and version | Not yet recorded | OPEN |
| PR Rust / CodeQL evidence | PR #128 + #129 passed; release workflow PR pending | PARTIAL |
| Manual Release workflow run URL, all platforms | Not run | OPEN |
| Linux packaged GGUF and default-seccomp real inference | Not run | OPEN |
| Windows packaged GGUF, inference, Job Object limits | Not run | OPEN |
| macOS packaged GGUF and documented sandbox limits | Not run | OPEN |
| Authentication, path confinement and manifest integrity | Source evidence exists; final adversarial acceptance pending | OPEN |
| ZIP advisory ignores and dependency exception review | Documented in `core-runtime/deny.toml` | REVIEWED, revisit upstream |
| GitHub private vulnerability reporting | Owner setting unverified | OPEN |
| Release artifact checksums and extracted executable tests | Not run | OPEN |
| Owner GO / tag / GitHub prerelease verification | Not approved | HOLD |

## Deliberate RC limitations

The following are legitimate later work, not grounds for deleting long-term product expectations: independent security audit, stable embedded public API contract [#112](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112), pinned downstream distribution [#113](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/113), integration conformance [#114](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/114), GPU and speculative performance qualification, and model encryption integrated into the loader. They must be described accurately and never implied to be RC-verified.

**Decision rule:** unresolved P0 or missing evidence = HOLD. If a P0 requires a code change, repair it on a focused PR and repeat the applicable acceptance tests. No premature `v1.0.0`, and no release merely because the PR queue is empty.
