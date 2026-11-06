# GG-CORE Wayfinder | Windows CPU baseline handoff v1

**Scope:** Read-only research assets for [Wayfinder local evidence ticket #156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156). This directory is on a **separate research branch**, not on GG-CORE main or the frozen published release. It is **not** a new release, source change, CI requirement, model download, or consumer-product qualification.

**Source authority:** frozen, immutable [GG-CORE v0.10.0-rc.1](https://github.com/MythologIQ-Labs-LLC/GG-CORE/releases/tag/v0.10.0-rc.1), exact commit fd9ee79574dfca8ba860b2e579bbce5555f0c6b0; accepted [RC gate #130](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/130). The tag must never move or be replaced.

## Checked-in inputs

- ggcore_wayfinder_fixtures_v1.py: **stdlib-only deterministic generator**. Original SHA-256 e74209f21cbf7eebec1e7a98c5be659cb0ca86b53622c4ddc3b2704e602e6c85; 4,258 bytes.
- ggcore_wayfinder_fixtures_v1.jsonl: exact six synthetic prompt records; SHA-256 **58e17222c7211d7af32f9bf01a85ed95e52d17a733754d4547be18825dad970f**; 17,289 bytes. The script independently verifies each prompt hash and its exact byte count.
- measure_windows_cpu_baseline.py: **optional stdlib-only Windows CLI round-trip probe**. It verifies the checked-in fixtures and published executable/model hashes before an explicitly approved execution. It does NOT start a daemon, verify Windows token isolation, report native TTFT/VRAM/peak RSS, grade answer quality, or run concurrency. Missing metrics remain null.

For the canonical exact prompt definitions and rubric, see [research resolution #151](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/151#issuecomment-6677806931). The published script and JSONL are static, synthetic-only material. Do not edit the prompts for tests and then pretend the old hashes apply.

## Admission and security gate FIRST

The owner authorizes *measurement planning and use of the already-approved 0.5B fixture* in a **genuinely isolated disposable Windows Dev environment**. Before any binary execution, the local Claude Code session must prove the effective **restricted Dev Windows token/SID and ACL denial against non-sensitive Stable canaries**, and ensure its IDE, shell, package tools and subprocesses remain under that restricted principal. Confirm no Stable data/keyring, production settings or credential paths will be touched. If this environment is not available or the test cannot safely run: **NO-GO** and report the exact blocker in [#156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156). Do not weaken restrictions or self-assert compliance simply by setting a flag.

Never use GG_CORE_ALLOW_INSECURE_NO_AUTH, GG_CORE_SANDBOX=off, GG_CORE_SECURITY_INGRESS=off or GG_CORE_SECURITY_EGRESS=off. Preserve real filesystem model confinement, manifest validation and integrity checks. The Windows Job Object is a resource/process boundary, **not** a filesystem/network/principal sandbox.

Do NOT run repository-local npm install, Cargo builds, untrusted project hooks or generated model-supplied code just to gather these measurements. Do NOT update CoreForge dependency pins or run its installed Stable product. Any required new model download, larger Qwen 7B/14B model or GPU experimental build requires separate owner permission.

## First pass: verify, do not execute

Use a trusted shell in the isolated Dev workspace. Use the Windows Python launcher if the Microsoft Store python3 alias is a stub.

~~~powershell
# Example paths only. Work in an isolated disposable Dev directory.
py -3 .\ggcore_wayfinder_fixtures_v1.py
py -3 .\measure_windows_cpu_baseline.py --fixtures .\ggcore_wayfinder_fixtures_v1.jsonl
~~~

Expected: PASS for six immutable fixtures, then VERIFY-ONLY. The fixture JSONL hash should be 58e17222c7211d7af32f9bf01a85ed95e52d17a733754d4547be18825dad970f.

Download the **actual published release assets**, not a newly built main branch:

~~~powershell
gh release download v0.10.0-rc.1 -R MythologIQ-Labs-LLC/GG-CORE --dir <ISOLATED_RELEASE_DIR>
py -3 .\measure_windows_cpu_baseline.py --fixtures .\ggcore_wayfinder_fixtures_v1.jsonl --release-dir <ISOLATED_RELEASE_DIR>
~~~

The verify-only script requires all three archives plus SHA256SUMS and refuses a mismatched digest. The published Windows archive SHA-256 must be **71abb8c8b23fe5a1d5719f3f97318a28df445466db4c680134da407de67af07c**. Extract with a Windows-compatible command (GNU tar may interpret C: as a remote host). The exact Windows gg-core-cli.exe binary SHA-256 is **69f78946ba5133cc39a69d20a7092a0d717bccd6e960cc758c945d271b13f402**. Those binaries are **unsigned**; the owner explicitly accepted the unsigned prerelease, not a signed Stable artifact.

The only initially admitted model is the official Qwen2.5-0.5B-Instruct Q4_K_M control file; sha256 **74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db**, Apache-2.0 metadata. Reuse an existing authorized model copy inside the isolated models root and verify the full hash. If not present, report the blocker; don't silently download other weights.

## Secured execution and measurement

First use the existing, frozen [producer RC acceptance script and IPC protocol](https://github.com/MythologIQ-Labs-LLC/GG-CORE/blob/v0.10.0-rc.1/scripts/rc-acceptance.sh) as the authoritative guide for starting an **already released** daemon, Windows named pipe and local token. Start from an isolated base with models/ confined under it and use a distinct pipe name, no Stable service or process. Configure native model context and matching admission limits:

- GG_CORE_N_CTX=4096
- GG_CORE_MAX_CONTEXT=4096
- GG_CORE_MAX_CONTEXT_TOKENS=4096
- CORE_AUTH_TOKEN: randomly generated, private and >=16 UTF-8 bytes, never printed or posted
- GG_CORE_SOCKET_PATH: unique local named pipe, not a CoreForge Stable endpoint

The optional Python probe does **not** start a daemon. With a verified running model, approved Dev admission, exact extracted published CLI path and model file path, use the following shell-neutral argument structure (replace placeholders locally; never post secrets):

~~~text
py -3 measure_windows_cpu_baseline.py
  --fixtures <PATH_TO_VERIFIED_JSONL>
  --release-dir <PATH_TO_RELEASE_ASSETS>
  --run --dev-isolation-approved
  --cli <PATH_TO_EXTRACTED_WINDOWS_GG_CORE_CLI.EXE>
  --model-file <PATH_TO_ALREADY_AUTHORIZED_HASHED_MODEL_GGUF>
  --model-id qwen
  --repetitions 3
  --out <NEW_LOCAL_ONLY_RESULTS.jsonl>
~~~

Join the arguments into one command or adapt to your shell. The --dev-isolation-approved flag is merely a human attestation; **it cannot prove Windows ACL, token or process-tree isolation**. The script requires Windows, verifies the CLI/model/fixture hashes, demands CORE_AUTH_TOKEN and GG_CORE_SOCKET_PATH in the environment, calls version --json, and then executes the six fixtures without logging raw model outputs or secrets. Results record actual subprocess round-trip milliseconds, exit code and hashed response only. **It does not produce native token-throughput, streaming TTFT, ITL, native peak RSS/VRAM, task-quality score, or reliable p99.** Ask Claude to collect those separately through authorized instrumentation, or mark them unmeasured.

## Decision-grade receipts for Wayfinder

Obtain actual CPU measurements, including: version/artifact/model/fixture hashes; Windows build and effective Dev identity (redacted SID proof); CPU/RAM, RTX driver and **actual free VRAM**; native context/sampling/thread settings; auth and sandbox state; baseline/peak/reclaimed memory if reliably observable; per-fixture latency and outcome rubric; warm/cold model load; failures and abort reasons; 1/4/8 competing requests only if controlled and supported. Run enough repetitions to characterize variance; **do not assert p95/p99 precision from three runs**.

Do not execute model-generated code on the Stable machine; evaluate candidate code in a separately qualified, disposable sandbox only. Preserve exact input hashes. Treat X01 security-policy blocking as a potentially correct outcome. Report success, genuine failures and missing evidence distinctly.

Deliver a redacted, attributable summary to [#156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156). Keep full local evidence in the restricted workspace; record a local bundle SHA and path and only upload sanitized artifacts when explicitly approved. Do not close #156 unless the empirical decision is actually supported or the issue's specific NO-GO criteria are met. The [backend strategy decision #154](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/154) remains blocked until this ticket supports a conclusion. The independent [transition HITL prototype #153](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/153) still awaits owner Q1–Q4 approval.

**Not authorized:** GG-CORE feature development, changes to frozen main/tag/assets, new releases, arbitrary model downloads, disabling security gates, GitHub Actions runs for this planning work, CoreForge app changes, or upgrading experimental GPU claims.