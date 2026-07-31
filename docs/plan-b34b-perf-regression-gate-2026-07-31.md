# Plan: B-34b — Run-over-Run Perf-Regression Gate

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Adds a run-over-run perf-regression gate to the CI `bench` job: cache the criterion baseline
    from `main`, restore it on PRs, and fail if any bench's median regresses > 2.0× (generous, for
    the trimmed-criterion variance regime). No source/bench-logic change; no committed absolute
    baseline (unsound per B-34 F4).
- non_goals:
  - No per-bench thresholds, no tightening below the gross-regression level, no `critcmp`/external
    service (YAGNI); no change to which benches run.
- exclusions:
  - No change to `lint`/`test`/`features` jobs; no Rust source change.

## Open Questions

None. Criterion emits `target/criterion/<group>/<id>/new/estimates.json` with
`median.point_estimate` (verified). `actions/cache` paths are repo-root-relative (so
`core-runtime/perf-baseline`), independent of the job's `working-directory`. First PR after merge
sees a cache miss → skip-and-pass until a `main` push seeds the baseline (documented, expected).

## Design Rationale (Simple Made Easy)

A committed absolute baseline is hardware-relative and unsound (B-34 F4). The sound gate compares
the same runner class run-over-run: `main` produces the baseline (cached), each PR restores it and
re-runs on `ubuntu-latest`, and a small script compares medians. The threshold is a function of the
measurement noise — the CI bench is short (2 s / 10 samples) and noisy, so the gate is a
gross-regression gate (2.0× / +100% median): non-flaky, and it still catches the failures that
matter (a reverted optimization, an accidental quadratic). Missing baseline never fails (absence is
not a regression).

## Phase 1: The comparison script

### Affected Files

- `core-runtime/scripts/perf_gate.py` (NEW) — compares two criterion trees:
  - `medians(root)`: walk `root` for `*/new/estimates.json`, key = path from `root` to the dir
    containing `new`, value = `median.point_estimate` (ns).
  - `main(baseline_dir, current_dir, threshold)`: if `baseline_dir` has no medians (cache miss /
    first run) → print a skip note and exit 0. Else, for each current bench with a baseline
    counterpart, compute `cur/base`; print a `base -> cur ns (xRATIO)` line flagged `ok` /
    `REGRESSION`; benches with no baseline print `NEW` (not a failure). Exit 1 iff any
    ratio > threshold; else exit 0. Argv: `<baseline_dir> <current_dir> <threshold>`.

### Unit Tests

No unit test (a CI-config/script change; the script's behavior is exercised by the CI gate step
itself). Local verification: `python3 core-runtime/scripts/perf_gate.py <dirA> <dirB> 2.0` against
two copies of `core-runtime/target/criterion` (identical → PASS, ratio 1.00; a hand-edited slower
copy → FAIL) confirms the compare + threshold + skip-on-missing logic without needing CI.

## Phase 2: Wire the gate into the CI bench job

### Affected Files

- `.github/workflows/rust.yml` — in the `bench` job, around the existing `cargo bench` step
  (paths in `actions/cache` are repo-root-relative; `run:` steps inherit `working-directory:
  core-runtime`):
  - BEFORE the bench, on PR only — restore the baseline:
    ```yaml
    - name: Restore perf baseline (PR)
      if: github.event_name == 'pull_request'
      uses: actions/cache/restore@v4
      with:
        path: core-runtime/perf-baseline
        key: criterion-baseline-${{ runner.os }}-${{ github.sha }}
        restore-keys: |
          criterion-baseline-${{ runner.os }}-
    ```
  - AFTER the bench, on PR only — gate:
    ```yaml
    - name: Perf regression gate (PR)
      if: github.event_name == 'pull_request'
      run: python3 scripts/perf_gate.py perf-baseline target/criterion 2.0
    ```
  - AFTER the bench, on push-to-main only — refresh the baseline for future PRs:
    ```yaml
    - name: Save perf baseline (main)
      if: github.event_name == 'push'
      run: cp -r target/criterion perf-baseline
    - name: Cache perf baseline (main)
      if: github.event_name == 'push'
      uses: actions/cache/save@v4
      with:
        path: core-runtime/perf-baseline
        key: criterion-baseline-${{ runner.os }}-${{ github.sha }}
    ```
  - The existing `Upload criterion baseline` artifact step is unchanged.

### Changes

Workflow YAML + one Python script. No Rust source change.

## Feature Inventory Touches

Empty — justified. CI-infrastructure change (a perf gate); no user-touchable runtime feature is
introduced or modified.

## Definition of Done

### Deliverable: a non-flaky run-over-run perf-regression gate

- **D1**: PRs to `main` fail CI when a tracked bench's median regresses > 2.0× vs the cached `main`
  baseline; a cache miss (no baseline yet) skips-and-passes; the baseline refreshes on each `main`
  push. No committed absolute baseline.
- **D2**: `core-runtime/scripts/perf_gate.py` (median compare, threshold, skip-on-missing) +
  `rust.yml` `bench`-job steps (restore/gate on PR, save on push) as above.
- **D3**: META_LEDGER entries (canonical markup) research #174, plan, audit, seal; BACKLOG B-34b →
  done (add the row); CHANGELOG note.
- **D4**: Locally, `python3 core-runtime/scripts/perf_gate.py A B 2.0` returns 0 for identical
  criterion trees and 1 for a >2× slower copy, and prints the skip note for an empty baseline dir.
  CI: on the PR-to-main run the `Perf regression gate (PR)` step executes and concludes success
  (cache miss → skip-pass on the first PR; real compare once `main` has seeded the cache).

## CI Commands

- `python3 core-runtime/scripts/perf_gate.py <dirA> <dirB> 2.0` — compare two criterion trees (identical→pass, slower→fail, empty baseline→skip)
- `cargo bench --bench inference_latency -- --warm-up-time 1 --measurement-time 2 --sample-size 10` — produce a criterion tree to test the script against
