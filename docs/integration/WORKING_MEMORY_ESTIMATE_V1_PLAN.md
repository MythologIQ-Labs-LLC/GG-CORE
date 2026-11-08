# Per-request working memory derived from model metadata (v1.0.0)

**Risk grade: L2/L3.** This changes request admission and the meaning of a resource limit. **Gate: a `/ql-audit` PASS is required before implementation.**
**Base:** #174 head `ab83e90`. This is a stacked branch, `feat/v1-working-memory-estimate-20261108-no-ci`.
**Closes:** the failed-allocation retention recorded on #174 and #161 (2026-11-08).

## Defect

A GGUF request reserves the operator-set `GG_CORE_MAX_MEMORY_PER_CALL` (1 GiB by default). That figure is unrelated to the context llama.cpp actually allocates for the request: its KV cache and compute buffers, at the daemon's `n_ctx`.

| Model at `n_ctx` 4096 | KV cache (measured) | Compute (measured) | Real need |
|---|---|---|---|
| Qwen2.5-0.5B | 48.00 MiB | 309.52 MiB | 358 MiB |
| SmolLM2-1.7B | 768.00 MiB | 120.02 MiB | 888 MiB |
| Phi-3.5-mini | 1536.00 MiB | 96.64 MiB | 1633 MiB |

When the reservation is below the real need, admission passes but llama.cpp then hits the Windows Job Object cap mid-allocation.

If the failure happens after the KV cache was allocated, the vendored llama.cpp (`llama-cpp-sys-2 0.1.161`) keeps about 845 MB committed until the process exits. This was reproduced identically on `160ba61` and `3c10a02`. Containment holds; availability degrades.

## Design

### 1. One context shape, used for both the parameters and the estimate
- Add a `ContextShape { n_ctx, n_ubatch: 512, kv_type: F16, flash_attn: Enabled }` value, built from `GgufConfig`.
- `LlamaBackendInner::create_context` builds `LlamaContextParams` from it, pinning `with_n_ubatch`, `with_type_k`/`with_type_v` (F16) and `with_flash_attention_policy(Enabled)`.
- These values match what llama.cpp already resolves today on CPU: `flash_attn = auto` is logged as "Flash Attention enabled", `n_ubatch` is 512 and the KV cache is f16. Inference behaviour is unchanged.
- The estimate can then never drift from the parameters actually used.

### 2. Estimate, computed once at model load from GGUF metadata (`llama-cpp-2` accessors)
- **Inputs:**
  - `n_layer`, `n_embd`, `n_head`, `n_head_kv` and `n_vocab`;
  - `n_ff` from `{general.architecture}.feed_forward_length`;
  - the head dimensions, from `{arch}.attention.key_length` / `value_length` when present, otherwise `n_embd / n_head`.
- **Formulas:**
  - `kv = n_layer × n_ctx × n_head_kv × (head_k + head_v) × 2 bytes`
  - `compute = n_ubatch × (n_vocab + n_ff + 2·n_embd) × 4 bytes`
  - `estimate = ⌈(kv + compute) × 1.125⌉ + 32 MiB` (output buffer, graph metadata, threads)
  - All arithmetic is `checked_*`. Overflow is treated as missing metadata (fail closed).
- **Validation against the measured anchors:**
  - KV is exact for all three families: 50,331,648 / 805,306,368 / 1,610,612,736.
  - Compute comes to 309.75 / 120.00 / 90.62 MiB against measured 309.52 / 120.02 / 96.64.
  - The estimate is 434 / 1031 / 1862 MiB against a real need of 358 / 888 / 1633, a ratio of 1.14–1.22.
- **Missing or implausible metadata** (no `n_ff`, `n_head` of 0, overflow): fall back to a conservative upper bound, `n_head_kv = n_head` and `n_ff = 8 × n_embd`.
  - The estimate is never *lower* than the formula on known values.
  - Sliding-window or hybrid caches are over-estimated as full caches, which is safe.

### 3. Admission
- `Model` gains `fn working_memory_estimate(&self) -> Option<usize>`. The default is `None`. `GgufGenerator` returns its load-time estimate; ONNX models return `None` (their per-call fallback still applies).
- `worker::acquire_guard` (generation, streaming and typed paths alike) reserves `estimate` when present, otherwise `max_memory_per_call`. The reservation still uses the checked `try_reserve` against the shared total, so resident plus working memory stays within the total, and therefore within the Job Object cap.
- **`GG_CORE_MAX_MEMORY_PER_CALL` becomes an optional operator ceiling:**
  - When it is **set explicitly** and a model's estimate exceeds it, the request is refused before llama.cpp with an explicit error naming both values and the variable.
  - When it is **unset**, there is no separate per-call ceiling: the total bounds everything.
  - **Owner decision required:** today's 1 GiB default would refuse every Phi-3.5 request (estimated need 1.86 GiB), which works now. The recommended default is "unset", meaning bounded by the total.
- **Process baseline:** at startup the daemon reserves 128 MiB of the total for its own baseline (measured idle private memory is 88–94 MB), so that the accounted total tracks the Job Object charge.

## Security and containment review
- **No limit is removed:**
  - The total and the Job Object cap are unchanged.
  - An explicit operator per-call ceiling still binds.
  - Admission becomes strictly *more accurate*: requests that would have failed inside llama.cpp are refused before it allocates.
- **Fail-closed:** missing metadata produces a larger estimate, never a smaller one, and overflow produces the conservative fallback.
- **No new input surface:** the metadata is read from an already admitted, path-confined, no-follow and hash-checked model through existing `llama-cpp-2` accessors. No new dependency.
- **Inference is unchanged:** the pinned parameters equal today's resolved values. The speculative-decoding path (feature `advanced`, not in the release feature set) creates extra contexts; it is out of scope and documented.

## Tests
- **Unit (no model file):**
  - The estimate function against the three measured shapes: estimate ≥ measured and ≤ 1.35 × measured.
  - The missing-`n_ff` fallback is at least the formula.
  - Overflow inputs produce the fallback.
  - `key_length`/`value_length` overrides are used.
- **Worker:**
  - It reserves the model estimate rather than `max_memory_per_call`.
  - `None` falls back to the per-call value.
  - An explicit ceiling below the estimate gives an explicit refusal that names `GG_CORE_MAX_MEMORY_PER_CALL`, and the counter is unchanged.
  - Two concurrent requests reserve twice the estimate within the total.
- **Context parameters:** `create_context` uses the `ContextShape` values, asserted through a params builder unit.
- **Real models (offline Hyper-V guest):**
  - The 1.74 GB `failalloc` scenario becomes an explicit admission refusal with **0 retained bytes**.
  - Default limits: all five models resident, the three families answer correctly, unload and reload behave as before.
  - For every inference, peak private bytes stay at or below resident private plus the estimate.

## Out of scope
- Fixing or patching vendored llama.cpp. Report it upstream with the reproduction.
- GPU memory.
- Per-request `n_ctx`.

## Razor
- The estimate goes in a new `engine/gguf/working_memory.rs` (pure functions plus tests); `ContextShape` lives there too.
- `backend.rs` is already 678 lines, a Razor violation that predates this change and is recorded here but not addressed by it. This change must not grow it: the `LlamaContextParams` construction in `create_context` moves into `working_memory.rs` (`ContextShape::params()`), so the net change to `backend.rs` is 0 or fewer lines.
- Functions stay at or under 40 lines, and nesting at 3 or less.
