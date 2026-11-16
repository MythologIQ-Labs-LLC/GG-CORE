# GG-CORE v1.0.0 typed ONNX inference: implementation and evidence status

**Status (2026-11-16): Implemented in main source; current-v1 packaged/installed qualification is incomplete.** This corrects the original, now-stale source-only foundation description. Do not call the frozen v0.10.0-rc.1 release an ONNX-serving product, or mistake a merged implementation for an approved v1.0.0 model/platform support matrix.

**Source authorities:** [#171 typed serving](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/171), [#160 supported capability matrix](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160), [#161 integrated release gate](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161), and [root architecture index](../../ARCHITECTURE_INDEX.md). GG-CORE is an upstream standalone inference runtime. CoreForge is an optional downstream consumer.

## What is implemented in current source

| Component | Source | Implemented behavior |
| --- | --- | --- |
| Typed engine | core-runtime/src/engine/inference_typed.rs | Backend-neutral embedding, embedding batch and text classification with explicit task/model/input/output verification and bounded results |
| IPC protocol | core-runtime/src/ipc/protocol_typed.rs | Additive typed request/response within existing JSON V1; closed task/result types, structured errors and optional negotiated feature |
| Authenticated handler | core-runtime/src/ipc/handler_typed.rs | Shared admission, timeout/cancel/shutdown controls and explicit refusal; not a direct ONNX bypass |
| Worker pipeline | core-runtime/src/scheduler/worker_typed.rs | Shared bounded queue, ingress screening on all texts, memory admission and filtered/text-safe egress |
| Standalone CLI | core-runtime/src/cli/typed_cmd.rs | Authenticated embed and classify commands through IPC, without local/ungoverned model execution |
| Real IPC integration test | core-runtime/tests/typed_ipc_test.rs | Windows named pipe and actual Candle ONNX execution on a tiny synthetic fixture: 401/closed unauthenticated, golden vector/batch, wrong-task and injection refusal |

The source path was built through merged PRs [#172](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/172), [#173](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/173), [#174](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/174), and [#175](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/175). The newer #49 capability snapshot is informational and cannot turn an untested model into a released support claim.

## Historical real-model evidence, not exact-current-main qualification

The [November 7 offline Windows Sandbox receipt](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/171#issuecomment-6700630814) reports license- and hash-verified real MiniLM-L6-v2 embeddings and DistilBERT SST-2 classification, served through authenticated typed IPC in an isolated Windows guest:

- MiniLM embedding batch returned 384-dimensional finite vectors and the intended semantic similarity ordering.
- DistilBERT returned the expected positive/negative labels and high confidence.
- Unsupported task and incorrect authentication were refused; unload/reload worked.
- Multiple GGUF model families were swapped while both ONNX models remained resident; working-set reclamation was measured under explicit raised limits.
- Historical evidence source was a local integration build, not a published stable v1 artifact. The same receipt identified a default-memory policy defect and retained working set after a failed allocation.

A [follow-up memory-admission receipt](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/171#issuecomment-6701144320) reports successful model runs under corrected default accounting. It is still evidence for its recorded integration identity, not automatic proof for every later merged runtime commit or operating system.

## What remains required

1. Re-run pinned, licensed real MiniLM and DistilBERT typed inference against the **current exact producer source and packaged/installed binaries** under default production auth, security and resource settings. Verify model/tokenizer/labels/hash, output semantics, task refusal, privacy and provenance.
2. Establish actual Windows/Linux/macOS support cells with the approved operating-system/feature matrix. The original Windows real-model receipt does not prove Unix/macOS execution or a public wheel/SDK.
3. Qualify failed-load cleanup, concurrent model admission, long-run behavior, cancellation/deadline/stream interactions, GGUF backward compatibility and memory limits on real models. Source unit tests and synthetic fixtures alone cannot provide these receipts.
4. Track secure A-to-B-to-A switching, actual RSS/mmap release, readiness and recovery under [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137). Do not duplicate that implementation under #171.
5. Align capability and version negotiation with the [#112 embedded contract](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112), and add real downstream conformance under [#114](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/114). Never treat ONNX embeddings/classification as ONNX text generation; the latter remains separately governed by [#115](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/115).

**Security and authority boundary:** All inference remains offline-first, authenticated over local IPC, and behind the existing scanner/admission/egress controls. The daemon has no implicit network, downloader, filesystem expansion, tool-use, or cloud fallback authority. Valid model imports must be staged by an independently authorized host component.

**Release disposition:** #171 stays open for current-v1 installed acceptance. The frozen v0.10.0-rc.1 release and CoreForge consumer pins remain unchanged. No CodeQL result, passing source test or historic sandbox run alone authorizes v1.0.0 publication.
