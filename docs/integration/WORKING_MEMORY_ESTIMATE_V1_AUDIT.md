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
