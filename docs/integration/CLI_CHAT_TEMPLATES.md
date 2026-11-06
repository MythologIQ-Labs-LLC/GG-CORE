# GG-CORE v1.0.0 development: explicit Qwen2.5 chat prompt format

**Status:** draft CLI feature, not qualified against a real instruct model, **not** complete per-model template support. [Owner-approved v1.0.0 Q4](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/159#issuecomment-6690938391); [implementation issue #167](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/167).

## Why this exists

The [Windows CPU baseline #156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156#issuecomment-6690245052) ran the Qwen2.5-0.5B-Instruct GGUF through the existing `infer --prompt` raw completion mode. All six fixture outputs failed the quality rubric. The model was loaded and execution succeeded; **it is not yet established whether ChatML or the model's small size explains those failures**.

Qwen2.5 Instruct expects model-specific chat role delimiters. The new CLI option **explicitly** wraps a single user prompt, with an optional system message, into the Qwen2.5 ChatML form and sends it across the **unchanged authenticated IPC channel**. No network listener, model download or daemon trust expansion is introduced.

## Operator usage

```shell
# Existing behavior, backwards compatible (raw completion):
gg-core-cli infer --model qwen --prompt "What is the capital of France?" --max-tokens 32

# Explicit Qwen2.5-Instruct chat turn:
gg-core-cli infer --model qwen --chat-template qwen2.5 --prompt "What is the capital of France?" --max-tokens 32

# Optional role-separated system instruction (still one user turn):
gg-core-cli infer --model qwen --chat-template qwen2.5 --system "Answer briefly" --prompt "What is the capital of France?" --stream
```

No template is selected automatically. **Do not apply `qwen2.5` to another model architecture simply because it shares the same GGUF file extension**. `--system` without a named chat template is rejected. This initial feature accepts one CLI user message plus an optional system message, not multi-turn history or true model-owned role objects.

The formatter emits:

```text
<|im_start|>system
(optional instructions)<|im_end|>
<|im_start|>user
(user prompt)<|im_end|>
<|im_start|>assistant
```

For the system-less case, omit the entire system turn. The last assistant delimiter is deliberately left open for generation. Reserved model-token introducers (`<|`) in the supplied user or system text are rejected rather than allowing untrusted text to impersonate role tokens. Raw completion mode intentionally does **not** interpret or filter these tokens; the daemon's existing ingress/egress checks still apply to both paths.

## Build and acceptance

Development branch source unit tests: exact known Qwen2.5 ChatML formatting, system-user order, Unicode/multiline content, raw-mode compatibility, injection refusal, blanks, unsupported template. **These tests require an actual Rust toolchain and have not been executed in this authoring environment.**

Qualify with an isolated offline Windows/Linux build: `cargo fmt --check` and `cargo test --locked --bin gg-core-cli chat_template` under the `core-runtime/` working directory, then `cargo test --locked --features gguf` where supported. Use the exact authorized Qwen 0.5B model SHA `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db`. Re-run the [six pinned fixtures](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156) under **identical parameters**, with the original raw path and the new named chat path; check real answers, injected-role rejection, long-context behavior, streaming terminals, model-token counts and wall-clock performance. If the daemon's current sanitization strips trusted ChatML control tokens, diagnose without disabling ingress or outbound safety checks.

Use a separate model-family scope and acceptance table for **all** v1.0.0 models. This first CLI feature is NOT sufficient to declare [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) complete. A full v1.0.0 runtime must derive correct templates from trusted model metadata/explicit owner policy and enforce role boundaries in **both** streaming/nonstreaming and embedded SDK paths. The current [`GgufGenerator::format_chat_prompt`](https://github.com/MythologIQ-Labs-LLC/GG-CORE/blob/fd9ee79574dfca8ba860b2e579bbce5555f0c6b0/core-runtime/src/engine/gguf/generator.rs) uses hard-coded non-Qwen delimiters and is **not** changed by this CLI-only patch.

**Security:** Model-specific chat formatting controls inference framing, not the authority to run tools, read arbitrary files, expose HTTP, transmit data, install dependencies or pick models. Preserve networkless IPC, sandbox, auth, validation and egress across every evaluation.
