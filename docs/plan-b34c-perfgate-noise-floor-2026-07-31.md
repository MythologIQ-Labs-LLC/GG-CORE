# Plan: B-34c — Perf-Gate Noise Floor for Sub-Microsecond Benches

**change_class**: hotfix

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Adds a ~1 µs noise floor to `perf_gate.py`: benches whose baseline median is below the floor are
    reported but never FAIL the gate (their CI variance exceeds the 2.0× threshold). Benches at or
    above the floor gate unchanged. Fixes the observed PR #101 false-positive flake.
- non_goals:
  - No change to the global threshold (stays 2.0×), the CI workflow, or which benches run; no bench
    logic change.
- exclusions:
  - No Rust source change.

## Open Questions

None. The failure mode + fix are grounded in the PR #101 flake (research #185): sub-µs
`concurrent_resource_ops` benches swung 2.3–2.4× on runner jitter; a re-run passed unchanged.

## Design Rationale (Simple Made Easy)

The gate must be tuned to its measurement noise, and the noise is not uniform: at
`--measurement-time 2 --sample-size 10`, timing below ~1 µs is dominated by scheduler jitter, so a
2× threshold on those benches is meaningless and flaky. Keep the gate's teeth where the signal is
reliable (µs+) and demote sub-floor benches to report-only. A flaky gate gets disabled; a scoped one
stays trusted.

## Phase 1: Add the noise floor to `perf_gate.py`

### Affected Files

- `core-runtime/scripts/perf_gate.py`:
  - Add `NOISE_FLOOR_NS = 1000.0` (module constant, documented: below this, CI-trimmed criterion
    variance exceeds the threshold).
  - In the compare loop, before the ratio check: if `base_ns < NOISE_FLOOR_NS`, print
    `  noisy      {key}: {base}->{cur} ns (x{ratio}; < {NOISE_FLOOR_NS:.0f}ns floor — not gated)` and
    `continue` (do NOT append to `regressions`). Benches with `base_ns >= NOISE_FLOOR_NS` keep the
    existing `REGRESSION`/`ok` logic and gate at the CLI `threshold`.
  - Update the module docstring to note the noise floor.

### Unit Tests

No unit test (a CI-config script; verified by direct invocation). Local verification: build two
synthetic baselines from `target/criterion` — (a) a sub-floor bench made 3× slower must PASS
(floored, reported `noisy`); (b) a ≥floor bench made 3× slower must FAIL (gated). Plus identical
trees → PASS, empty baseline → skip (unchanged).

## Feature Inventory Touches

Empty — justified. CI/measurement-infrastructure refinement; no user-touchable runtime feature.

## Definition of Done

### Deliverable: noise-floored perf-gate (no sub-µs flakes; teeth intact above the floor)

- **D1**: the perf-gate no longer FAILs on benches whose baseline median < 1 µs (reported as
  `noisy`); benches ≥ 1 µs still gate at 2.0×. The PR #101 flake class cannot recur.
- **D2**: `NOISE_FLOOR_NS = 1000.0` + the report-not-fail branch in `perf_gate.py`.
- **D3**: META_LEDGER entries (canonical markup) research #185, plan, audit, seal; BACKLOG note
  (B-34c done; folds the B-34b flake).
- **D4**: local runs — sub-floor 3×-slower → exit 0 (PASS, `noisy`); ≥floor 3×-slower → exit 1
  (FAIL); identical → exit 0; empty baseline → exit 0. CI: the `bench` job's gate step runs green
  (and stops flaking on `concurrent_resource_ops`).

## CI Commands

- `python3 core-runtime/scripts/perf_gate.py <baseline_dir> <current_dir> 2.0` — sub-floor regressions report `noisy` (pass); ≥floor regressions FAIL
