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

## Revision 2 (2026-11-08): remediation of audit VETO V1–V4

These rules **supersede** the corresponding text above.

### V1: always fail closed on attention mode
- The compute estimate **always** includes the attention-score term, whether or not flash attention ends up enabled:

  `compute = n_ubatch × (n_vocab + n_ff + 2·n_embd) × 4 + 2 × n_ubatch × n_ctx × n_head × 4`

  The factor 2 covers both the attention-score matrix (KQ) and its softmax output, which can be live at the same time in the non-flash graph.

- This is the bound for flash attention disabled, so whatever llama.cpp resolves (auto probe, forced off for Grok, a backend that lacks the op) the estimate is at or above the real need. With flash attention enabled it over-reserves by that term: 224 MiB for Qwen and 512 MiB for SmolLM2 and Phi-3.5 at `n_ctx` 4096 (before margin).
- `ContextShape` still pins `n_ubatch = 512` and f16 K/V, the two inputs the formula depends on. Flash attention stays **auto**: the estimate no longer depends on it, so inference behaviour is untouched.
- **Revised anchors** (estimate vs measured real need):

  | Model | Estimate | Measured | Ratio |
  |---|---|---|---|
  | Qwen | 686 MiB | 358 MiB | 1.92 |
  | SmolLM2 | 1607 MiB | 888 MiB | 1.81 |
  | Phi-3.5 | 2438 MiB | 1633 MiB | 1.49 |

- **Unit tests:** the estimate is at least the measured flash-attention-on need for all three anchors, and at most 2.25 × that need.
- **Measured flash-attention-off acceptance.** This is required, because the flash-attention-off bound is analytic.
  - An `#[ignore]` real-model integration test (`gguf` feature, Windows) assigns its own process to a fresh Job Object, through the existing `Win32_System_JobObjects` feature.
  - It loads each anchor model, records `PeakProcessMemoryUsed`, creates a context with flash attention **disabled** (a test-only `ContextShape` override) and runs one prefill. It then asserts that the peak delta is at or below `working_memory_estimate`.
  - It runs natively in the offline VM against the three pinned models.
  - **If any model's peak delta exceeds the estimate, implementation does not ship:** the margin must be raised and the test re-run.

### V2: owner decision recorded (2026-11-08)
**`GG_CORE_MAX_MEMORY_PER_CALL` unset means no separate per-call ceiling; the total bounds everything.**
- **No struct change.** `ResourceLimitsConfig` is unchanged; a new field would break every existing struct literal, including any external Rust embedder's.
  - The ceiling lives in the shared `LimitsInner` as a `OnceLock<usize>`, so every clone already held by the scheduler, worker and lifecycle sees it.
  - It is set at most once, through `ResourceLimits::set_per_call_ceiling(&self, usize) -> Result<(), AlreadySet>`. Unset means no ceiling.
  - `config::EnvConfig` gains `per_call_ceiling: Option<usize>`, which is `Some` only when `GG_CORE_MAX_MEMORY_PER_CALL` is explicitly present. `EnvConfig` is loader output, constructed only in `config.rs`.
  - The standalone daemon's `runtime_init` calls `set_per_call_ceiling` right after `Runtime` construction and before the IPC server starts or any preload runs.
  - FFI, Python and programmatic embedders never call it, so they have no ceiling.
- `try_acquire` applies its per-call refusal only when the ceiling is `Some`.
- **Existing field `max_memory_per_call`:** it becomes the **fallback working reservation**, for models without an estimate (ONNX, or GGUF where the estimate is unavailable).
  - Its default stays 1 GiB, so existing struct users compile and behave as before.
  - When the variable is set, it equals the ceiling, as today.
- **Reservation per request:**
  - **with an estimate:** `estimate`, refused explicitly if it exceeds a `Some` ceiling;
  - **without one:** `max_memory_per_call`.

### V3: CLI and config surface
- `config_cmd::run_defaults` prints:
  - `GG_CORE_MAX_MEMORY_PER_CALL=unset (no per-call ceiling; bounded by GG_CORE_MAX_TOTAL_MEMORY; fallback reservation 1073741824)`;
  - `GG_CORE_MAX_TOTAL_MEMORY=max(2147483648, 50% of physical RAM)`, which also corrects the stale #174 value.
- `run_validate` warns on total < per-call **only when the ceiling is explicitly set**.
- The `config.rs` table and docs are updated to match.
- **Tests:** the defaults output strings, and validate with the ceiling unset or set.

### V4: baseline reservation dropped
The 128 MiB process-baseline reservation is removed from scope. Recorded as a residual: the daemon's idle private memory (88–94 MB measured) is not budgeted. It is covered in practice by the flash-attention-off over-reservation above, but that is not a guarantee.
