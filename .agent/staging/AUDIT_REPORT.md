# AUDIT REPORT (rev.3 NARROW RE-AUDIT)

**Date**: 2026-07-08T16:51 (session 2026-07-08T1651-6c68b6, iteration 3)
**Target**: docs/plan-runtime-hardening-cycle2-2026-07-08.md (rev.3) — Phase 2 site map + 3 oracle reworks
**Risk Grade**: L3 (touches src/security/ files; validation seam)
**Prior**: VETO rev.1 (Entry #82), VETO rev.2 (Entry #83). rev.2 residual defects F1 (health.rs:94,95 mis-lint) + F2 (stats.rs:61,66 mis-lint).

## Verdict: PASS

The two rev.2 blocking defects (F1, F2) are RESOLVED. All 13 rows of the Phase 2
site map now match source ground truth. The 3 oracle reworks are sound Rust (all
referenced symbols are compile-time `const`). No new scope creep or assertion
loss. F-35 test_path corrected to encryption_tests.rs. Implementation authorized.

Merkle chain intact at Entry #83 head; recompute consistent. No lock condition.

---

## 13-Row Ground-Truth Confirmation

| # | Site | Plan lint label | Source truth | Correct? |
|---|------|-----------------|--------------|:---:|
| 1 | thread_pool_tests.rs:3 | unused import | `use super::super::thread_pool_types::*;` at :3; downstream uses `super::*` (:4). Plausible unused glob. | Y |
| 2 | lifecycle_tests.rs:19 | new_ret_no_self | `fn new(...) -> Arc<dyn GgufModel>` at :19 — ctor named `new` not returning Self. | Y |
| 3 | persistence.rs:196 | field assign outside Default | `let mut state = RegistryState::default();` (:195) then `state.saved_at = ...` (:196). | Y |
| 4 | persistence.rs:274 | field assign outside Default | `let mut state = RegistryState::default();` (:273) then `state.saved_at = 1234567890` (:274). | Y |
| 5 | persistence.rs:336 | field assign outside Default | `let mut state1 = RegistryState::default();` (:335) then `state1.default_model = ...` (:336). | Y |
| 6 | persistence.rs:341 | field assign outside Default | `let mut state2 = RegistryState::default();` (:340) then `state2.default_model = ...` (:341). | Y |
| 7 | prompt_injection.rs:189 | unnecessary same-type cast (u8→u8) | `severity as u8 * 5` at :189; `classify_severity` returns `u8` (:235), `severity` bound at :180. No-op cast in live scan(). | Y |
| 8 | stats.rs:61 | manual checked division | `total_latency_us / successes / 1000` at :62 inside `if successes > 0` guard opened at :61. Manual-checked-division pattern. | Y |
| 9 | stats.rs:66 | manual checked division | `total_tokens / successes` at :67 inside `if successes > 0` guard opened at :66. Manual-checked-division pattern. | Y |
| 10 | encryption_tests.rs:371 | assertion on constant value | `assert!(ModelEncryption::PBKDF2_ITERATIONS >= 600_000)` at :371 over a `const u32`. Constant assertion. | Y |
| 11 | health.rs:94 | assertion on constant value | `assert!(EXIT_HEALTHY == 0, ...)` at :94 over `pub const i32` (:15). Constant assertion. | Y |
| 12 | health.rs:95 | assertion on constant value | `assert!(EXIT_UNHEALTHY != 0, ...)` at :95 over `pub const i32` (:16). Constant assertion. | Y |
| 13 | ipc_client.rs:404 | io::Error::other suggested | `std::io::Error::new(std::io::ErrorKind::Other, "test")` at :404. `io_other_error` lint. | Y |

**13/13 lint labels correct.** No mis-attribution survives.

---

## Prior-VETO Site Resolution (F1 / F2)

- **F1 RESOLVED** — health.rs:94,95. rev.2 falsely labeled these "manual checked
  division / checked_div". rev.3 rows #11/#12 now correctly label them "this
  assertion has a constant value" and rework to `const _: () = assert!(...)`.
  Source has NO division; the lint is now the true constant-assertion class.
- **F2 RESOLVED** — stats.rs:61,66. rev.2 falsely labeled these
  "field-assignment-outside-of-initializer". rev.3 rows #8/#9 now correctly label
  them "manual checked division" and target the two `X / successes` divisions
  inside their `if successes > 0` guards. Matches source exactly.

The two labels were swapped in rev.2 (division-lint pinned to the assertion sites,
default-lint pinned to the division sites); rev.3 unswaps both correctly.

---

## 3 Oracle-Const Soundness Checks (#10, #11, #12)

`const _: () = assert!(EXPR)` requires EXPR to be a const-evaluable bool. Each
referenced symbol confirmed `const`:

- **#10 PBKDF2_ITERATIONS** — `pub const PBKDF2_ITERATIONS: u32 = 600_000;`
  (encryption_core.rs:117). `>= 600_000` is const-evaluable. SOUND — compiles;
  guarantee strengthened to compile-time, not deleted.
- **#11 EXIT_HEALTHY** — `pub const EXIT_HEALTHY: i32 = 0;` (health.rs:15).
  `== 0` const-evaluable. SOUND.
- **#12 EXIT_UNHEALTHY** — `pub const EXIT_UNHEALTHY: i32 = 1;` (health.rs:16).
  `!= 0` const-evaluable. SOUND.

All three are `pub const` primitives in module scope; the `const _` items compile
and the asserts fail compilation if the guarantee is ever violated (honest gate).

---

## Regression / Scope / Honesty Checks

- **No new scope creep** — Phase 2 still 13 fixes across 8 files; exclusions
  (ipc/, sandbox/, no security/ behavior change) intact. security/ edits remain
  behavior-preserving (const-assert rework #10 + no-op cast removal #7).
- **No assertion loss** — #10/#11/#12 relocate test-body asserts to `const _`
  items (strictly stronger). Prior-PASS validate_path relocations unchanged.
- **F-35 CORRECTED** — test_path now `core-runtime/src/security/encryption_tests.rs`
  (was fips_tests.rs in rev.2). Confirmed at plan:92.
- **Test honesty** — reverting the mechanical sites (#1-9,#13) re-triggers clippy
  under `-D warnings` (gate fails); reverting #10-12 to runtime `assert!` restores
  the lint AND the const guarantee is enforced at compile time. Honest gates.
- **Prior-PASS dimensions not re-litigated** — #58 removal, validate_path
  substance (#57/#55), Section 4, forbidden modules/deps: unchanged in rev.3, not
  re-audited per narrow mandate.

---

## New Findings

None blocking. Minor (non-veto, already flagged rev.2 as fold-in, not
re-litigated here): integration-test line drift is a Phase 1 detail outside this
narrow Phase-2 re-audit scope and does not affect any load-bearing claim.

---

## Disposition

**PASS.** Implementation of Phase 1 (validate_path #57/#55) and Phase 2 (clippy
residuals #56) is AUTHORIZED. Proceed to `/ql-implement`.

## Merkle Seal
Chain intact at Entry #83 head. This re-audit appends a PASS entry (#84).
No Shadow Genome entry (VETO cleared).
