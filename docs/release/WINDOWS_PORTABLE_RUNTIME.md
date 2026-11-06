# GG-CORE Windows clean-install dependency qualification (v1.0.0 development)

**State: DRAFT source/build fix for [#162](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/162), not compiled, shipped, signed or clean-image accepted.** Preserve frozen [v0.10.0-rc.1](https://github.com/MythologIQ-Labs-LLC/GG-CORE/releases/tag/v0.10.0-rc.1) untouched. Do not use this documentation as a claim that the Windows package already works.

## Scope and security rule

The portable Windows archive must launch with no previously installed Microsoft VC++ Redistributable. The known released build imports `MSVCP140.dll`, `VCOMP140.DLL`, `VCRUNTIME140.dll`, and `VCRUNTIME140_1.dll` from `gg-core-cli.exe` and `gg_core.dll`. On a pristine Windows Sandbox the frozen package fails with STATUS_DLL_NOT_FOUND, per [Windows evidence #156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156#issuecomment-6690245052).

**Proposed implementation:** app-local, unmodified Microsoft-signed x64 runtime DLLs staged **only** from an authorized licensed Visual Studio `VC/Redist/MSVC/<version>/x64` tree, alongside the binary. This is an implementation **proposal**, not a final owner decision between app-local redistribution, installer prerequisite chaining and static CRT. It preserves a portable, air-gappable archive without requiring a live installer or internet. No DLLs are copied from `System32`, other developers' profiles, user machines, untrusted downloads or debug/nonredistributable folders.

The script validates the original Microsoft Authenticode certificate and full SHA-256 of each DLL **before and after** copying, and emits `MSVC_REDISTRIBUTABLES.json` with source-toolchain version, redistributable family, Microsoft identity, file version and digest. The verifier checks each packaged hash/signature and executes `gg-core-cli.exe version --json` with `gguf` required. The published artifact remains unsigned until independently authorized Azure Artifact Signing; Microsoft DLLs must retain original publisher signatures.

**Licensing dependency:** distribution of individual Visual C++ runtime files requires the applicable **licensed Visual Studio rights and Microsoft software license terms**; verify redistribution permission, toolchain edition, file eligibility, notice and obligations before shipping. See [Microsoft redistributing documentation](https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files) and [Microsoft's Visual Studio redistribution list](https://learn.microsoft.com/visualstudio/releases/2026/redistribution). The script cannot assert your organization holds those rights.

**Serviceability:** Microsoft recommends central `vc_redist.x64.exe` installation for ongoing library servicing. App-local copies will not be serviced through the host's standard central runtime updates. Therefore, v1.0.0 maintenance must monitor Microsoft advisories and qualify patched GG-CORE app-local redistributable files, or the owner may choose an installer-chain strategy instead. A truly frozen binary is not exempt from security patches.

## New build paths

- `scripts/stage-windows-msvc-runtime.ps1`: discover approved redistributable root (prefer `VCToolsRedistDir`; fallback licensed `vswhere` installation), stage the four required signed x64 DLLs, record digest provenance, refuse ambiguous/unsigned/non-Microsoft sources.
- `scripts/verify-windows-msvc-runtime.ps1`: verify exact four library names, SHA-256 and Microsoft Authenticode signatures, and test the packaged executable can start with GGUF.
- `.github/workflows/release.yml`: **future branch code only** stages those files after Windows GGUF build, packages them with the archive and metadata, and re-extracts/verifies the Windows archive before upload. Existing Linux/macOS paths are unchanged. No Actions run was triggered by this draft.

## Required independent qualification before merge and future release

1. **Toolchain and legal review:** verify source redist edition/licensing, authorized Visual Studio redistributable folder, each Microsoft signature, runtime architecture, license notices and exact file SHA. Confirm whether `Microsoft.VC143.OpenMP` / `Microsoft.VC145.OpenMP` and CRT family layout matches the actual chosen toolchain. Fail if missing; never copy unsafely from System32.
2. **Inspect import closure:** use `dumpbin /dependents` or PE import-inspection tooling on **both** `gg-core-cli.exe` and `gg_core.dll` on each candidate build; compare all non-OS dependencies, not merely the four frozen RC imports. If more libraries (for example `msvcp140_1.dll`) are introduced, extend the explicit trusted packaging manifest and tests before promotion. No silent scavenging.
3. **Build verification:** local Windows `pwsh` execution of staging script and verifier, exact CLI version/feature checks, package extraction/round-trip, intentional missing/mismatched/forged DLL negative tests. No arbitrary external download or bypass for successful tests.
4. **Real pristine guest:** start a fresh **offline Windows Sandbox/VM**, with networking, clipboard, GPU and host-writable shares off; mount candidate archive **read-only**, then copy it into guest-controlled temporary storage for extraction. No VC++ Redistributable installed and no host DLL PATH changes; verify `version --json`, `--help`, native library import/FFI, authorized secure model load, real inference and default sandbox/auth under this exact extracted package. Record guest OS/build, exact binary and redistributable hashes and package SHA; **zero STATUS_DLL_NOT_FOUND**.
5. **Confinement:** guest has no access to host Stable data/canaries or credentials; no network dependency and no secrets in archived evidence. Preserve the accepted independently verified Windows Sandbox isolation proof, but produce a new proof for the candidate.
6. **Failure and robustness:** absent/tampered redist, missing license receipt, wrong x86/x64 DLL, blocked Microsoft signature or incompatible version must fail the **build/qualification**, never silently accept another developer's installed runtime. Check antivirus/signing interoperability and offline package update/rollback.
7. **Signing and finalization:** independently verify Azure Artifact Signing of **GG-CORE-owned executable/DLL/installers** before claiming signed release. Microsoft-signed redistribution files are *not* re-signed. Clear outstanding [#163 named-pipe concurrency](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/163) and full v1.0.0 integrated gates separately.

**GO/NO-GO:** A passing runner on `windows-latest` is only a source/package-integrity receipt. The #162 defect is resolved **only after a pristine offline Windows guest independently starts and infers from the whole candidate package and its licensing/signature/upgrade path is owner accepted.** Keep #162 and this PR OPEN/DRAFT until then.

No tag or release publishing is authorized by this patch.
