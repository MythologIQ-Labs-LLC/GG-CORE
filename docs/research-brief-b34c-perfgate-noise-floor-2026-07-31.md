# Research Brief — B-34c: Perf-Gate Noise Floor for Sub-Microsecond Benches

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-34c — harden `perf_gate.py` (B-34b) so it does not FAIL on sub-microsecond benches,
whose CI variance exceeds the 2.0× threshold. Triggered by an observed false-positive flake.
**Scope**: `core-runtime/scripts/perf_gate.py`.

---

## Executive Summary

The B-34b perf-gate fired a **false positive** on PR #101 (an advanced-gated change that provably
cannot affect the flagged code): `concurrent_resource_ops/sequential/{5,10,20}` — sub-µs
`ResourceLimits` benches (~83→193, 154→372, 321→737 ns) — crossed the 2.0× threshold at ~2.3–2.4×,
purely from CI runner jitter; a re-run passed 13/13 unchanged. This is exactly B-34b F4's predicted
failure mode: at `--measurement-time 2 --sample-size 10`, benches below ~1 µs have >2× run-to-run
variance, so a 2× gate on them cries wolf. Fix: add a **noise floor** — report sub-floor benches but
do not FAIL on them; gate only benches whose baseline median is large enough to time reliably in CI.

## Findings (verified)

### F1 — the flake is sub-µs benches, not the changed code
- PR #101 (B-21b-1, advanced-gated, cannot touch `memory/limits.rs`) failed the gate solely on
  `concurrent_resource_ops/sequential/{5,10,20}` at 2.30–2.41×. All are sub-µs (`ResourceLimits::
  try_acquire`, `benches/memory_overhead.rs`). Every µs+ bench was `ok`. A `--failed` re-run on a
  fresh runner passed all 13 checks with no code change → the "regression" was measurement noise.

### F2 — the 2.0× threshold is below these benches' CI noise floor
- B-34b F4 shipped the 2.0× threshold as "generous, tunable from observed CI variance." This is the
  first observed variance datum: sub-µs benches swing ~2.4× between runners at the CI-trimmed
  criterion settings. So 2.0× is *not* generous enough for them; a threshold below the noise floor
  produces flakes, and a flaky gate gets ignored — worse than no gate.

### F3 — the fix is a noise floor, not a higher global threshold
- Raising the global threshold (e.g. 3.0×) would blind the gate to real regressions on the reliable
  µs+ benches. The right move is to **skip the FAIL decision for benches whose baseline median is
  below a noise floor** (~1000 ns), where timing is dominated by scheduler jitter, while still
  printing them (visibility) and still gating everything above the floor at 2.0×. The gate keeps its
  teeth where measurement is trustworthy.

## Recommendations

1. **B-34c deliverable**: add `NOISE_FLOOR_NS = 1000.0` to `perf_gate.py`; in the compare loop, a
   bench with `base_ns < NOISE_FLOOR_NS` is printed as `noisy … (not gated)` and never added to
   `regressions`, regardless of ratio. Benches ≥ floor gate unchanged at the CLI threshold. Document
   why (CI-trimmed criterion variance).
2. Locally verifiable: run `perf_gate.py` against a synthetic baseline where a sub-floor bench is
   made 3× slower (must PASS — floored) and a ≥floor bench 3× slower (must FAIL — gated).

## Updated Knowledge (Shadow Genome)

**Reinforces B-34b's Shadow Genome**: a perf gate must be tuned to its measurement noise — and the
noise is not uniform. Sub-microsecond benches at short CI measurement times are un-gateable; gate
only where the signal exceeds the jitter, and report the rest. The first real flake is the tuning
datum, not a reason to abandon the gate.

---

_Research complete. B-34c = a ~1 µs noise floor in `perf_gate.py` (report-not-fail below it). Fixes
the observed #101 false positive; keeps the gate meaningful above the floor. No behavior change to
gated benches._
