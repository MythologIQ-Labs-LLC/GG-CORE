# Research Brief — B-34b: Run-over-Run Perf-Regression Gate

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-34b — the timing-regression gate deferred from B-34: fail CI when a tracked bench
regresses beyond a tuned threshold, using run-over-run baselines (not a checked-in absolute
baseline, which B-34 F4 proved unsound). Closes the optimization initiative.
**Scope**: `.github/workflows/rust.yml` bench job + a small comparison script; no source change.

---

## Executive Summary

B-34 established a CI `bench` job over 10 CI-safe benches and proved a committed absolute-timing
baseline is unsound (hardware-relative). The sound gate is **run-over-run on the same runner
class**: cache the criterion baseline produced on `main`, and on each PR restore it, re-run the
benches on the same `ubuntu-latest` runner, and fail if any bench's median regresses beyond a
threshold. The criterion data already exists (`target/criterion/<group>/<id>/new/estimates.json`,
`median.point_estimate` in ns). Because the CI bench run is deliberately short
(`--measurement-time 2 --sample-size 10`) its variance is high, so the threshold must be
**generous** — a gross-regression gate (≥ ~2× / +100% median) that catches algorithmic regressions
(a dropped optimization, an O(n)→O(n²)) without flaking on noise. Recommendation: `actions/cache`
the `main` baseline (keyed by runner OS, rotating by sha with a prefix restore-key), a
`scripts/perf_gate.py` that compares medians per bench, and workflow steps that SAVE on push-to-main
and GATE on PR. It is CI-only verifiable (cache mechanics don't exist locally), so one tuning
iteration on real CI runs is expected.

## Findings (verified)

### F1 — criterion data is present and machine-readable
- Each bench writes `core-runtime/target/criterion/<group>/<bench_id…>/new/estimates.json` with
  `{"median": {"point_estimate": <ns>}, "mean": {…}, …}` (verified locally). The `bench_id` may be
  nested (e.g. `chat/10_messages`). The comparison key is the path between `criterion/` and
  `/new/estimates.json`; the metric is `median.point_estimate` (robust to outliers vs mean).

### F2 — the workflow triggers support the save/gate split
- `rust.yml` triggers on BOTH `push: [main]` and `pull_request: [main]` (`:3-7`); the `bench` job
  runs `working-directory: core-runtime` and already uploads `target/criterion` as an artifact.
  `github.event_name` distinguishes `push` (→ save the baseline) from `pull_request` (→ restore +
  gate). No trigger change needed.

### F3 — the sound baseline is run-over-run via cache, keyed by runner OS
- `actions/cache` created on `main` is readable by PRs targeting `main`. Key
  `criterion-baseline-${{ runner.os }}-${{ github.sha }}` with `restore-keys:
  criterion-baseline-${{ runner.os }}-` gives each main push a fresh entry and lets a PR restore
  the most recent one. Same runner class (`ubuntu-latest`) ⇒ comparable timings (B-34 F4's
  soundness condition). Restore the main baseline into a SEPARATE dir (`core-runtime/perf-baseline`)
  so the PR's own bench run (`target/criterion`) doesn't clobber it; compare the two trees.

### F4 — the threshold must be generous (trimmed criterion = high variance)
- The CI bench uses `--warm-up-time 1 --measurement-time 2 --sample-size 10` (B-34's CI-budget
  trim) → wide confidence intervals (observed CIs in this session's runs span ±10–30% on the
  faster benches). A tight threshold (+15–30%) would flake. A **gross-regression** threshold
  (ratio ≥ 2.0, i.e. +100% median) is non-flaky and still catches the failures that matter (a
  reverted optimization, an accidental quadratic). Ship it generous and documented as tunable;
  tighten only if longer baselines are later adopted. Missing baseline (cache miss / first run) or
  a bench with no counterpart ⇒ skip-and-pass with a log (never fail on absence).

### F5 — self-contained; no new cargo tool
- A ~40-line `scripts/perf_gate.py` (python3 is on `ubuntu-latest`) walking both criterion trees is
  more controllable than `critcmp` (which reports a table but has no threshold exit-code), and adds
  no `cargo install` CI time. It prints a per-bench old→new median table and exits non-zero on the
  first regression over threshold.

## Blueprint Alignment

| B-34 seed for B-34b | Finding | Status |
|---|---|---|
| Run-over-run baseline via cache + comparison | actions/cache keyed by OS, rotating sha (F3) | MATCH |
| Threshold tuned to observed CI noise | trimmed criterion → generous gross-regression gate (F4) | MATCH |
| No unsound committed absolute baseline | baseline lives in cache, never in the repo | MATCH |

## Recommendations

1. **B-34b deliverable**: `scripts/perf_gate.py` (median compare, threshold, skip-on-missing) +
   `rust.yml` bench-job steps: on PR restore `perf-baseline` from cache then run the gate; on
   push-to-main copy `target/criterion`→`perf-baseline` and save it to cache. Threshold constant =
   `2.0` (+100% median), documented as deliberately generous for the trimmed-criterion regime.
2. **CI-only verification** (like B-16/B-33): implement → push → confirm on the PR run that the
   gate step executes (restores a baseline once one exists, compares, passes on no-regression).
   The FIRST PR sees a cache miss (no main baseline yet) → skip-and-pass; the gate goes live once a
   main push has seeded the cache. State this plainly in the seal.
3. **Do NOT** tighten the threshold speculatively or add per-bench thresholds now (YAGNI) — one
   global generous gate first; refine only with evidence.

## Updated Knowledge (Shadow Genome)

**A perf gate must be tuned to its own measurement noise.** The gate's threshold is a function of
how the benches are run in CI (here: short measurement → high variance → a gross-regression gate).
A gate tighter than its noise floor flakes and gets disabled; a documented generous gate that
catches 2× regressions is worth more than a precise one nobody trusts.

---

_Research complete. B-34b = run-over-run cache baseline + a median-compare `perf_gate.py`, generous
(+100%) threshold, save-on-main / gate-on-PR. CI-only verifiable; closes the optimization pass._
