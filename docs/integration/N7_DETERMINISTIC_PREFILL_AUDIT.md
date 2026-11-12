# AUDIT REPORT

**Tribunal Date:** 2026-11-12
**Target:** `docs/integration/N7_DETERMINISTIC_PREFILL_PLAN.md` (Revision 1), sha256 `944563803b2def802064c1a9d8d9141cb868a82bb919e650f136d4f8341b86bf`
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge (independent subagent, read-only)
**VERDICT:** VETO. The finding is narrow: it concerns the tests, not the design.

## Executive summary
The design holds against the source:
- **Coverage:** every plain prefill reaches `decode_prompt` (backend.rs:144, :360), including the embedded `generate`/`generate_text` path, the daemon, FFI and Python.
- **No carried state:** each request gets a fresh context, and there is no prompt-cache path.
- **One ubatch per chunk:** `N_UBATCH` is 512, so any chunk of 512 tokens or fewer is a single ubatch.
- **Determinism:** a pure `chunk_len(cap, n_threads, model_bytes)` removes every timing input.
- **Abort callback:** the claim is confirmed. `LlamaContext::context` (context.rs:29) and `context_params` (params.rs:302) are both `pub(crate)`, and the crate exposes no abort callback.

## Passes
- **Security: PASS.** No IPC, protocol or auth change, and no `unsafe`.
- **Ghost paths: PASS.** Speculative and verify keep `decode_range`, and the exclusion is consistent.
- **Razor: PASS.** The moved block is about 166 lines, which leaves backend.rs at about 575. It was already over the limit, and `generate_stream` (48) and `verify_tokens` (66) are existing over-limit functions.
- **Dependencies: PASS.**
- **Orphans: FAIL (minor).** The plan does not say how `backend_cancel_latency_tests.rs` (`PREFILL_CHECKPOINT_TOKENS`, `PrefillPacer::with_budget`, `decode_paced`) or the `mod prefill_pacer` declaration change.
- **Macro architecture: PASS.**

## Violations
| # | Location | Defect |
|---|---|---|
| V1 | plan step 5 vs backend.rs:508-514 | A "stop callback sleeps 50 ms" arm does not simulate load. The pacer starts its timer *after* `stop_requested`, so that test would also have passed against the C2 pacer. |
| V2 | plan step 5 vs latency tests :202-205 | The plan says the bounded-tail test asserts `tokens_generated == 0`. It does not; that assertion is in backend_cancel_tests.rs:101. The tail test also passes by skipping when the request is not `Cancelled` or the fixture is missing. |
| V3 | plan step 5 | `GG_CORE_REQUIRE_REAL_MODEL` does not exist yet. The plan applies it to the new test only, so the other real-model tests still return early without failing. |
| V4 | plan step 5 | The formula "stop checks == ceil(tokens/chunk)" is wrong for `generate_cancellable`, which also makes one check before prefill and one per generated token. |

## Required remediation
1. Add a structural test. A `#[cfg(test)]` wrapper records the sequence of chunk lengths, and the test asserts it is identical with `None`, with `Some(never)` and with a timing-perturbed callback, and that it equals the schedule. For real load, apply contention *during* decode, not inside the callback.
2. Fix the tail test: state the real source, add `assert_eq!(r.tokens_generated, 0)`, and under the require gate turn its skips into failures.
3. Define `GG_CORE_REQUIRE_REAL_MODEL=1` once, in a shared fixture helper, and use it in every touched real-model test.
4. Use the exact count formula, or assert the count at the `decode_chunked` level.
5. List the rewrite of the latency and characterization tests.

## Non-blocking observations
- **Cross-host determinism:** an `n_threads`-dependent chunk makes output vary across hosts and worker counts, which fixed 512 did not. d184 showed thread count alone does not change logits. Consider a chunk that does not depend on thread count.
- **Embedded no-stop callers:** they change from the native 512 cap to the schedule. Record this as a compatibility note.
- **Cost proxy:** `model.size()` measures tensor bytes and over-penalises Q8/F16. `n_params()` exists as an alternative. A calibration on one model is unqualified for larger models.
- **GPU offload:** the CPU-derived estimate is too small. Document it or clamp it.
- **Wiring:** make the schedule a method on `LlamaBackendInner` so that `generate_stream` does not grow.
- **Concurrency test:** give the reference run `None` and update the "time-paced" comment.

---

# AUDIT REPORT: resubmission (Revision 2)

**Target:** plan sha256 `60e91ed620abd246333203a051fa3a6ee0d3290554d6ea287c33b4cba9b7b476`. **Risk grade:** L2. **Auditor:** The QoreLogic Judge (independent subagent). **Verdict:** VETO, on the plan text and formula only.

**Resolved:**
- V1: the recorded-chunk test now fails against the C2 pacer.
- V2, V3 and V4.
- The orphaned items.
- The position step is well defined.

## Violations
- **R1:** the scale formula is inverted, so larger models get full-size chunks. Snapping below 16 is undefined, and the truncation cliffs are not stated.
- **R2:** the tail claims for positions below 4096 do not follow from c184. Chunk cost rises linearly with position: a 128-token chunk at position 3968 takes about 2.0 to 2.1 s at 1 thread. The 1.5 s 1-thread gate is predicted to fail, and at default threads the 128-token chunk near position 4095 takes about 0.5 to 0.67 s, not 0.32 to 0.37 s.
- **R3:** the late-position throughput cost of 64 against 512 is +38% to +94% at 4 to 8 threads, not 24% to 32%.
- **R4:** the claim that 32 is slower than 64 at every thread count is false. At 8 threads, 32 is faster in absolute time.

## Residual conditions
1. The recorder must observe the production path through `prefill`.
2. Gate `real_model_long_prompt_beyond_batch` as well.
3. Verify the default worker assumption.

---

# AUDIT REPORT: resubmission (Revision 3)

**Target:** plan sha256 `4e1672abfc517d7332a50e544249203a0c7ff1a43d529094e644f386f35546a4`. **Risk:** L2. **Auditor:** The QoreLogic Judge (independent subagent). **VERDICT: PASS**, subject to the conditions below.

## Remediation check
- **R1:** fixed. Timing now scales with `model_bytes / REF`, the chunk floor is `min(64, cap)`, and the unit cases cover model sizes from 0.5x to 100x the reference.
- **R2/R3:** hold. The auditor ran `scripts/qualification/n7_prefill_schedule_model.py` against the c184 data, and every transition, worst-chunk time and cost figure matches the plan.
- **R4:** fixed. The per-token figures match the CAL lines.
- **Default threads:** verified in source. 12 cores divided among 2 workers gives 6 threads, which is bucket 4.
- **Earlier residual conditions:** all addressed.
- **Schedule purity:** the schedule is a pure function.
- **Acceptance gates:** falsifiable and set before measurement.

## Conditions (binding on implementation)
1. **Fixed-point integers.** Store the table as fixed-point integers (2x ms) with defined rounding, and use saturating or checked arithmetic throughout. The boundary tests must come from the same integer formula. If a transition shifts, regenerate the plan table; do not change the code to fit it.
2. **Underflow guard.** A test or const check must assert `T16 >= T0` for every size and bucket, so the subtraction cannot underflow.
3. **No retune loop.** If a gate fails, re-deriving constants needs a fresh calibration batch and a re-audit, and the original failure stays recorded for the owner.
4. **Embedded/daemon split.** Document that embedded `Runtime::infer` and the daemon run in different buckets on the same host (12 threads is bucket 8; 6 threads is bucket 4), so their greedy text can differ.
