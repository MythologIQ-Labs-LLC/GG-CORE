# Standalone multi-model packaged acceptance (v1.0.0 engineering tool)

**Status:** Independent acceptance-harness implementation under review; **no real GG-CORE daemon or model has yet been tested with this harness**. The harness's offline Python unit/mock-CLI suite does **not** qualify a model, build, operating system, GPU or release.

## Scope

Qualify **one pinned local model** on **one exact packaged GG-CORE CLI/daemon** per run, independently of any other application. Intended to repeat across the owner-approved GGUF model families and ONNX embedding/classification fixtures, on the supported OS/hardware matrix.

This complements rather than replaces `scripts/rc-acceptance.sh` (existing Qwen-oriented bundled GGUF and low-level IPC control tests). It is NOT a security penetration test, CPU/GPU benchmark, inference quality measurement, stop-latency bound, or evidence of cross-model switching.

The harness uses only Python 3.9+ standard library, and does not download models, call remote providers, create listeners, change the runtime source, write into `models/`, or disable sandbox/security controls.

## Usage

From a clean host with a locally installed packaged `gg-core-cli` and a model already staged under `<base>/models/`:

```sh
python3 scripts/standalone_model_acceptance.py \
  --cli /path/to/gg-core-cli \
  --cli-sha256 <64-hex-digest-of-packaged-cli> \
  --base /path/to/offline-workspace \
  --model models/smollm2.gguf \
  --model-sha256 <64-hex-known-good-weight-digest> \
  --model-id smollm2 \
  --commit <40-hex-commit-from-artifact-provenance> \
  --task generation \
  --output standalone-smollm2.json
```

Substitute `--task embedding` or `--task classification` for a licensed and supported ONNX fixture with its own model manifest. Both invoke the actual typed `embed`/`classify` CLI command and validate the returned result shape. This is meant to **expose** incomplete task serving rather than silently accept a model that merely loads.

`--chat-template qwen2.5` is optional and applies **only** to a Qwen2.5 fixture. No unsupported model template is inferred for another architecture. The harness verifies a nonempty generation/stream, not semantic response quality. Quality requires separate scored prompts and official family-specific templates.

## Real acceptance steps

1. Require explicit 64-hex expected CLI and model hashes; compare to actual files before launch; reject symlinks or paths outside `base/models`.
2. Require `version --json` source commit and compiled backend features to agree with the declared fixture/task. Record binary hash, model hash, target and protocol version.
3. Clear inherited `GG_CORE_*` config, generate a fresh local high-entropy auth token and unique socket/pipe address; rely on **platform default sandbox**, never force `off` or `ALLOW_INSECURE_NO_AUTH`.
4. Run `config validate`, start the packaged daemon with the pinned fixture, and wait with a bounded deadline for readiness.
5. Check `live`, `status --json`, `models list --json`, model identity and advertised capability.
6. Run actual inference and streaming (GGUF generation), or one typed JSON embedding/classification; verify nonempty result and, for typed results, finite numeric values and exact shape.
7. Check that a wrong authentication token cannot infer, unload the model, verify readiness drops, reload it, verify readiness and successful inference again; ensure the daemon remains alive.
8. Terminate the private daemon, without modifying the weights or interfering with a user's preexisting IPC endpoint. Report outcome without raw prompt text, output text, credential or daemon logs.

For each supported fixture/OS/accelerator cell, record **exact model family and license**, file hash, CLI hash/commit, host hardware, runtime feature flags, result JSON, test date, OS containment status, context settings, and independently measured quality/performance/security evidence. Results from one Qwen fixture do not support claims about other GGUF models, ONNX, CUDA, Metal, Linux, or macOS.

## Development and release gates

- Run: `python3 -m unittest discover -s tests -p test_standalone_model_acceptance.py -v` (no model, network, CLI or VM required). The fake CLI subprocess test checks harness orchestration **only**.
- Run on an actual offline GG-CORE binary with locally licensed, pinned models; report exact executable digest and evidence receipt.
- Run existing `rc-acceptance.sh` separately for low-level IPC, model integrity and cancellation controls; this tool cannot replace it.
- Independent code review and merged state before the result becomes a standard acceptance gate.
- Only an owner-approved fully integrated `v1.0.0` capability/OS/model matrix and complete release gate can authorize product claims.

**Ownership:** GG-CORE standalone product only. No consumer integration code, no HTTP interface, no paid CI or public release is authorized by this test harness.
