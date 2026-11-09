# GG-CORE → CoreForge Dev: immutable Windows CPU/IPC integration checkpoint

**Plan ID:** CF-GGCORE-2026W46-C1  
**Target decision:** Friday, **2026-11-13** (America/New_York)  
**Owner intent:** resume CoreForge development against a **functional frozen GG-CORE producer snapshot this week**.  
**Status:** **PROPOSED, NOT FROZEN, NOT RELEASE-QUALIFIED.** A date is a target, never permission to waive acceptance.  
**Producer gate:** [GG-CORE #180](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/180)  
**Consumer adoption:** [CoreForge #758](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/758)  
**Installed consumer acceptance:** [CoreForge #548](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/548)  
**Full GG-CORE v1 contract:** [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) and [#161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161)

## 1. Version and scope boundaries

The checkpoint is an **internal, exact-source, exact-binary producer/consumer integration baseline** for **Windows x86-64 CoreForge Dev**. It is **not** a public GG-CORE `v1.0.0-rc.N` or stable release, a replacement for the published immutable `v0.10.0-rc.1`, a CoreForge Stable release, or a claim that the whole standalone v1.0.0 capability matrix is qualified.

- GG-CORE initial candidate **source baseline**: `ae55c97632d00079f435abb6d975ae8166d18315`, merged [#178](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/178) plus [#179](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/179). If necessary fixes are committed, use the **new exact reviewed source SHA**; do not continue to claim this original SHA.
- The **binary/archive SHA-256s are NOT KNOWN** until a controlled Windows build. Do not infer them from commit SHA, version text, earlier release archives or CI check conclusions.
- Frozen public `v0.10.0-rc.1` source: `fd9ee79574dfca8ba860b2e579bbce5555f0c6b0`; preserve its archived identity, CoreForge fallback and security checks.
- GG-CORE needs Rust `1.97+` per current manifest; CoreForge's own Windows Rust/Cargo toolchain is `1.95.0`. The **child-daemon interface** avoids embedding the producer crate into the CoreForge Cargo graph.
- No HTTP, no outbound downloads in the inference daemon and no authorization of cloud or tool execution. Model acquisition, when explicitly authorized, happens outside the daemon.
- CoreForge consumes only the capabilities qualified in its *actual installed* Dev flow. A library-only/feature-gated path or a successful source unit test is not evidence of installed product support.

## 2. Topology and authority

```text
CoreForge Dev UI (Settings, Alden, Alice)
         |
         | Tauri commands, selected persona/model, explicit user intent
         v
CoreForge controller (Rust 1.95)
  - Dev-only app/data/model namespace; no Stable state mutation
  - exact producer manifest + binary SHA-256 + path and signature admission
  - per-launch authenticated IPC token; process/pipe identity check
  - child lifetime, bounded cancellation/deadlines, restart/rehydration
  - Windows Job Object for resource/process bounds
         |
         | local authenticated Windows named pipe; protocol/capability negotiation
         v
GG-CORE producer sidecar (Rust 1.97+, Windows x64, CPU/GGUF)
  - verified executable and app-local Microsoft-signed runtime DLLs
  - resource admission, model file confinement/hash, secured model registry
  - generation/streaming, deadline/cancel, bounded queue and stream backpressure
  - ingress/egress security, typed failures, local health/readiness
         |
         | read-only licensed, SHA-256-pinned local model inputs
         v
Local GGUF weights (first acceptance: Qwen2.5-0.5B-Instruct Q4_K_M)
```

**Authority rules:**
1. The UI is not an authority for readiness. Report `verified` only after real authenticated selected-persona inference from the installed sidecar; distinguish process running, model loaded, model ready and successful inference.
2. The producer cannot grant itself CoreForge permissions. The controller owns policy and token distribution. Explicit `GG_CORE_N_THREADS` override remains available; use safe worker-aware auto defaults otherwise.
3. A Windows Job Object is **not** principal, filesystem or network isolation. Test offline/negative boundaries on the actual Windows environment, disclose remaining risk, and never claim a sandbox it cannot provide.
4. Validate the **actual launched** binary, not just staged bytes, and bind every acceptance receipt to the exact source/EXE/DLL/archive. Disallow symlink/junction/path replacement and mixed-version staged resources.
5. Stable keeps its own installed identity, pipe, model/data roots, updater policy and known binary; Dev may not overwrite or migrate it.
6. The **old RC1 verifier/stager** in CoreForge is immutable legacy evidence. New checkpoint selection is a separate explicit manifest/profile that fails closed on unknown identities, not a permissive modification to an allowlist.

## 3. Linear critical path

| Gate / date target | Required work | Owner issue | Passing receipt | Current status |
| --- | --- | --- | --- | --- |
| G0 / Mon Nov 9 | Architecture, exact scope, security and issue dependencies; inspect producer source and consumer stack ancestry | GG [#180](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/180), CF [#758](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/758) | Reviewed contract and branch ancestry, no unapproved scope/CI/tag | **IN PROGRESS** |
| G1 / Tue Nov 10 | Build actual Windows x64 GGUF producer, license/provenance package, clean offline startup; real model, authenticated pipe, 8 clients, deadline/cancel, restart, negative auth/path, observed memory | GG [#180](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/180) | Exact source, archive/EXE/DLL/runtime SHA-256s, Windows host/model/security receipts | **BLOCKED: local native and packaged proof** |
| G2 / Wed Nov 11 | Create separate CoreForge Dev candidate manifest/stager/identity profile; reconcile old stacked PR ancestry, Cargo 1.95 sidecar-only lock, Rust ↔ TS status/protocol contract, real GGUF persona-route consumer tests | CF [#758](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/758), [#542](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/542), [#543](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/543) | Locked native build, exact producer identity, source conformance, selected Alden/Alice model response; no Stable mutation | **BLOCKED BY G1** |
| G3 / Thu Nov 12 | Fresh CoreForge Dev NSIS, clean offline installation, real UI Alden/Alice inference, settings/first-run, restart, cancellation, over-cap and tamper refusal, Stable/Dev coexistence, uninstall/rollback | CF [#548](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/548), [#744](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/744) | Exact NSIS SHA-256 and installed, exercised outcomes on current producer/consumer commits | **BLOCKED BY G2** |
| G4 / Fri Nov 13 | Independent evidence/security/compatibility review; owner accepts or rejects immutable Dev snapshot and controlled merge/pin | GG [#180](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/180), CF [#758](https://github.com/MythologIQ-Labs-LLC/COREFORGE/issues/758) | GO only with complete G1–G3 receipts, rollback identity and accepted limitations | **BLOCKED BY G1–G3** |

**Dependency chain:** G0 → G1 → G2 → G3 → G4. Safe preparatory design, tests and build orchestration can run in parallel, but they **do not bypass** acceptance order. A failed gate triggers a specific remediation on an isolated branch and an exact-artifact retest; it never silently advances.

**Critical consumer branch concern:** existing CoreForge draft [#746](https://github.com/MythologIQ-Labs-LLC/COREFORGE/pull/746) is stacked under an RC1 feature branch; [#748](https://github.com/MythologIQ-Labs-LLC/COREFORGE/pull/748) is stacked onto #746; [#749](https://github.com/MythologIQ-Labs-LLC/COREFORGE/pull/749) onto #748. Their current hardcoded v0.10 hash guards are intentional. Inspect ancestry before selecting an integration base; never force-merge the stacked chain directly onto an incompatible `main`, rebase accepted history blindly, or overwrite the old RC1 fallback.

## 4. Required functional proof (snapshot only)

- Clean/offline Windows x64 without system-wide VC++ redist: the package's app-local MS-signed `MSVCP140.dll`, `VCOMP140.dll`, `VCRUNTIME140.dll` and `VCRUNTIME140_1.dll` match provenance/version/hash; executable passes `version --json`.
- A known-sha licensed GGUF file is imported/preloaded and responds to an **authenticated** real IPC request; no filesystem/network ambient permission is inferred from Job Object use.
- 8 simultaneous authenticated clients connect without unhandled `ERROR_PIPE_BUSY`; overload produces a bounded, explicit refusal. A non-reading streaming client times out/cancels without monopolizing worker loops.
- Negative cases: wrong token; wrong protocol/capability; wrong model/path/hash; frame/request-ID mismatch; canceled/expired requests; producer crash/restart; wrong executable/DLL; metadata/log/token leakage; failed installation and rollback.
- CoreForge **Dev installed** Settings, launch route and selected Alden/Alice persona actually get model output, including after restart; source-only inference does not count.
- CoreForge Stable and Dev coexist under one Windows login with separate process, IPC, model, identity, profile and updater state; no Stable mutation.
- Candidate tests may use the existing Qwen fixture for first consumer functionality. Other GGUF model families and typed ONNX remain important **v1 support-matrix work**, but their absence from CoreForge's first CPU/Alden/Alice consumer flow does not become an invented Dev snapshot requirement.

## 5. Qualification tuple and freeze rule

Record **one source of truth**:

```text
status: CANDIDATE | QUALIFIED | HOLD
producer_source_sha:
producer_features_and_protocol:
producer_archive_name_and_sha256:
producer_daemon_exe_size_and_sha256:
producer_ffi_dll_size_and_sha256:
microsoft_runtime_dll_versions_signatures_sha256:
model_id_source_revision_license_sha256:
windows_edition_build_arch_offline_state:
producer_security_ipc_concurrency_memory_test_receipts:
coreforge_source_sha_and_rust95_cargo_lock_receipt:
coreforge_dev_nsis_name_sha256:
actual_selected_alden_alice_outputs_and_restart_receipts:
stable_dev_isolation_and_negative_tests:
independent_reviewer_and_findings:
owner_go_no_go:
rollback_to_v0_10_0_rc1_identity:
```

**Freeze semantics:** before G4, a source SHA is a **candidate**, not a sealed dependency. At G4 GO, record immutable source and packaged artifact digests, preserve the archived bytes, update **Dev-only** exact pin/manifest in a governed PR, and prove installed consumer behavior on that exact pin. No public version tag, publication, v1.0.0 completeness claim, automated cross-repo pin change or CoreForge Stable promotion follows automatically.

**NO-GO:** unverified or mismatched hashes, incomplete source/consumer ancestry, failed or missing clean-installed inference, auth/security bypass, uncontrolled network, wrong persona/model, missed cancellation/timeout, Stable identity mutation, or unsupported OS/GPU claim. If Friday's GO fails, continue CoreForge on the established frozen RC1 fallback and report the precise remaining blocker.

## 6. Separate long-term GG-CORE track

Retain the complete standalone v1.0.0 capability/support decisions in [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) and full integrated release GO/NO-GO in [#161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161). Native CUDA and Metal, governed adaptive switching, exact broader model/task support, advanced inference/SDK, Linux/macOS installed security, Kubernetes observability, final documentation and signed public release artifacts require their own actual proof. This internal checkpoint **does not waive or defer their approved v1 scope**.

## 7. First executable handoff

Claude on the authorized **local Windows build VM** should start at **G1** once G0's exact ancestry/contract review is complete:

1. Read current `main`, release scripts, the known producer PR #178/#179 receipts, CoreForge #746/#748/#749 ancestry and this contract.
2. Build GG-CORE at **the exact candidate source SHA**, feature set including real GGUF, with the approved Windows Microsoft-signed app-local dependency path. **Do not trigger GitHub Actions or publish**.
3. Run an actual extracted portable archive in a pristine offline Windows sandbox with the pinned real Qwen fixture and the G1 negative/concurrency matrix. Capture and verify reproducible source, model, archive, EXE, DLL and runtime evidence.
4. Report exact failures or, if G1 passes, record its proof and move to G2's additive consumer profile. The next artifact should be **tested binaries and real receipts**, not another planning document.
