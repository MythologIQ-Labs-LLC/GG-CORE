# AUDIT REPORT

**Tribunal Date:** 2026-11-08
**Target:** per-request working memory from GGUF metadata, `docs/integration/WORKING_MEMORY_ESTIMATE_V1_PLAN.md` (sha256 `f5fa93ad96eab8a733c44f2139b4f3c5232f4b13776466166967a26b599ac55f`)
**Risk Grade:** L2/L3
**Auditor:** The QoreLogic Judge
**VERDICT:** VETO

## Executive summary
The estimate is well grounded: KV is byte-exact on three model families, and compute is within 6% of measurement. Four defects still block implementation:
- The plan's own fail-closed claim ("never lower") does not hold on a path llama.cpp actually takes.
- The binding semantics of a resource limit are left unresolved.
- Configuration reporting would diverge from runtime behaviour.
- A new budget reservation has no named owner.

## Passes
- **Security: FAIL (V1).**
  - The plan pins `flash_attn = Enabled` and estimates compute without the attention-score (KQ) term.
  - However, the vendored `llama_init_from_model` forces flash attention **off** for `LLM_ARCH_GROK` (`llama-context.cpp:3819`), and auto mode resolves it by a runtime probe (`:559`).
  - With flash attention off, compute gains about `n_ubatch × n_ctx × n_head × 4` bytes (268 MB for SmolLM2 at 4096) that the estimate omits.
  - Admission would then under-reserve and reach the exact Job Object failure path this plan is meant to close. No auth or credential issue.
- **Ghost UI: FAIL (V3).** `gg-core-cli config defaults` / `config validate` (`cli/config_cmd.rs`) hard-code `GG_CORE_MAX_MEMORY_PER_CALL=1073741824` and warn when total < per-call. The plan changes those semantics but does not cover this CLI surface, so it would report a default and a validation rule that no longer exist.
- **Razor: PASS.** The new pure module is sized well under the limits. `backend.rs` (678 lines, a pre-existing violation) gains no lines under the amended plan.
- **Dependencies: PASS.** None added; the accessors used (`n_layer`, `n_embd`, `n_head`, `n_head_kv`, `n_vocab`, `meta_val_str`, `with_n_ubatch`, `with_type_k`, `with_flash_attention_policy`) all exist in `llama-cpp-2 0.1.161`.
- **Orphans: FAIL (V4).** "At startup the daemon reserves 128 MiB": the plan names no owner (which component holds the `ResidentGuard`), no lifetime, and no behaviour for in-process FFI/Python embedding, where the process baseline belongs to the host.
- **Macro architecture: FAIL (V2).**
  - The default meaning of `GG_CORE_MAX_MEMORY_PER_CALL` is left as "owner decision required". The plan therefore does not define a single runtime behaviour.
  - Implementing either option without the decision makes the implementation, not the plan, the source of truth for a resource limit.

## Violations
| ID | Category | Location | Description |
|---|---|---|---|
| V1 | Security (containment) | Plan §1–2 | No handling when flash attention resolves or is forced off; the estimate can be lower than real need, contradicting the stated fail-closed guarantee |
| V2 | Macro | Plan §3 | The per-call default semantics are undecided; the plan specifies two incompatible behaviours |
| V3 | Ghost path | `cli/config_cmd.rs` | Config defaults and validation output are not updated for the new semantics |
| V4 | Orphan | Plan §3 "Process baseline" | Baseline reservation without an owner, lifetime or embedding behaviour |

## Required remediation
1. **V1:** make the estimate fail closed whenever flash attention is not guaranteed. Either:
   - always include the KQ term `n_ubatch × n_ctx × n_head × 4`; or
   - pin flash attention, refuse to estimate (use the conservative non-flash bound) for architectures llama.cpp forces off, and verify at context creation that the resolved setting matches. Add a unit test for the non-flash case.
2. **V2:** obtain and record the owner's decision on the `GG_CORE_MAX_MEMORY_PER_CALL` default (unset ⇒ bounded by total, or a numeric default), and write one behaviour into the plan.
3. **V3:** add `config_cmd` `run_defaults` / `run_validate` (and `config.rs` docs) to scope and tests, consistent with the chosen semantics.
4. **V4:** specify the owner (for example `ModelLifecycle`, created in the daemon's `runtime_init` only), its lifetime, and that FFI/Python embedding does not reserve it. Or drop the baseline reservation.

**Incidental finding (not part of this verdict; affects #174):** `config_cmd::run_defaults` still prints `GG_CORE_MAX_TOTAL_MEMORY=2147483648`, which has been stale since #174 changed the default to max(2 GiB, RAM/2).

## Ledger
Deferred to merge (parallel v1 branches). A Shadow Genome entry is appended on this branch.

---

# AUDIT REPORT: resubmission (Revision 2)

**Tribunal Date:** 2026-11-08
**Target:** `WORKING_MEMORY_ESTIMATE_V1_PLAN.md` with Revision 2 (sha256 `8f3c50d75c76dca756312653f8d00a50554456b932639631daf1c68f4849cb72`)
**Risk Grade:** L2/L3
**Auditor:** The QoreLogic Judge
**VERDICT:** PASS

## Disposition of the prior violations
- **V1: resolved.**
  - The compute estimate now always carries the non-flash attention bound, `2 × n_ubatch × n_ctx × n_head × 4` (scores plus softmax), so no flash-attention resolution can lower the real need below the estimate.
  - Because that bound is analytic, a **measured** flash-attention-off acceptance test (Job Object `PeakProcessMemoryUsed` on the three pinned models) is a ship-blocking gate.
  - `ContextShape` pins only the inputs the formula depends on (`n_ubatch` and f16 K/V); inference behaviour is unchanged.
- **V2: resolved.** The owner decided on 2026-11-08 that unset means no per-call ceiling, bounded by the total.
  - There is one behaviour: `OnceLock` ceiling state in the shared `LimitsInner`, set only by the daemon from an explicitly present environment variable, before serving.
  - No public struct changes, so existing `ResourceLimitsConfig` literals (FFI/Python, external embedders) still compile, and they have no ceiling.
- **V3: resolved.** `config_cmd` defaults and validation, plus `config.rs` docs, are in scope and tested. The stale #174 total default is corrected there.
- **V4: resolved by removal.** The process-baseline reservation is dropped; the unbudgeted daemon idle memory (88–94 MB) is recorded as a residual.

## Passes
- **Security: PASS.**
  - No limit is removed; the total and the Job Object cap are unchanged.
  - Every uncertainty (missing metadata, overflow, flash-attention mode) resolves to a larger reservation.
  - An explicit operator ceiling still binds.
  - No new input surface.
- **Ghost UI: PASS.** The CLI config surface is covered.
- **Razor: PASS.**
  - New pure module `engine/gguf/working_memory.rs`.
  - `backend.rs` gains no net lines; it is a pre-existing 678-line violation, recorded and not worsened.
  - Functions at most 40 lines, nesting at most 3.
- **Dependencies: PASS.** None added. The `windows-sys` features used (`Win32_System_JobObjects`) already exist.
- **Orphans: PASS.**
  - `working_memory.rs` is wired from `engine/gguf/mod.rs`.
  - `Model::working_memory_estimate` is consumed by the single `worker::acquire_guard`, which already serves the generation, streaming and typed paths.
  - The ceiling setter is called from the daemon's main between `Runtime::new` and preload.
- **Macro architecture: PASS.**
  - Single sources of truth: `ContextShape` for both the parameters and the estimate; `LimitsInner` for the ceiling.
  - Layering is preserved: memory has no engine imports, and the engine reports estimates through the `Model` trait.

## Ledger
Deferred to merge (parallel v1 branches). Shadow Genome entry #3: remediation applied in Revision 2.
