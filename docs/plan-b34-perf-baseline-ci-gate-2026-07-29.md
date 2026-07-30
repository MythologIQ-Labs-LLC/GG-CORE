# Plan: B-34 — Perf Baseline + CI Bench Job

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - This ships the **smoke + baseline** gate: the 7 CI-safe benches run in CI and the job
    fails on compile error / bench panic; `target/criterion` is uploaded as the baseline
    artifact. It does NOT yet fail on a timing-regression threshold — that is B-34b (a
    committed absolute baseline is unsound; the threshold must be tuned to observed CI
    noise, per research F4).
- non_goals:
  - No benchmark logic changes; no optimization; no `critcmp`/cache/threshold wiring (B-34b).
  - Model/GPU/advanced benches stay out of CI (need fixtures/hardware/feature).
- exclusions:
  - No change to `test`/`clippy`/`fmt` jobs.

## Open Questions

None. Deliverable split (smoke+baseline now, threshold gate as B-34b) resolved at cycle
start, grounded in the hardware-relative-baseline soundness constraint (research F4).

## Design Rationale (Simple Made Easy)

The day-one risk is **bench-rot** — benches exist but nothing runs them, so they can
silently stop compiling (as happened to `e2e_model_test`). A CI job that runs the CI-safe
benches and fails on error/panic closes that, and its criterion output is a reproducible
CI baseline. The timing-regression threshold is a separate, harder problem (hardware-
relative baselines; noise tuning) deliberately deferred to B-34b so it isn't guessed.

## Phase 1: Add a `bench` job to the CI workflow

### Affected Files

- `.github/workflows/rust.yml` — add a `bench` job (ubuntu-latest, same triggers as the
  workflow: PR/push to `main`) that:
  - checks out, installs the stable Rust toolchain, restores the `Swatinem/rust-cache`,
  - runs the 7 CI-safe benches explicitly with trimmed criterion timing for CI budget:
    ```yaml
    - name: Perf benches (default-feature, CI-safe set)
      run: |
        cargo bench -p gg-core \
          --bench ipc_throughput --bench scheduler_throughput --bench concurrent_load \
          --bench memory_overhead --bench kv_cache_throughput \
          --bench generation_throughput --bench inference_latency \
          -- --warm-up-time 1 --measurement-time 2 --sample-size 10
    - name: Upload criterion baseline
      if: always()
      uses: actions/upload-artifact@v4
      with:
        name: criterion-baseline-${{ github.sha }}
        path: core-runtime/target/criterion
        if-no-files-found: warn
    ```
  - The explicit `--bench` list (research F3) avoids compiling the gpu/advanced/model bench
    targets. The `--` args trim criterion's warm-up/measurement/sample counts so the job
    fits the CI budget while still exercising every bench. A compile error or bench panic
    fails the step (the gate); the upload records the baseline.

### Changes

Workflow YAML only. No Rust source change.

### Unit Tests

- No unit test (CI-config change). Local verification: each of the 7 benches compiles and
  runs to completion under default features with the trimmed criterion args (`cargo bench
  --bench <name> -- --warm-up-time 1 --measurement-time 2 --sample-size 10`) on the dev
  host; CI verification: the new `bench` job runs green on the PR-to-main.

## Feature Inventory Touches

Empty — justified. CI-infrastructure change (a benchmark job); no user-touchable runtime
feature is introduced or modified.

## Definition of Done

### Deliverable: CI `bench` job over the CI-safe bench set

- **D1**: The 7 CI-safe benches run in CI on every PR/push to `main`; the job fails if any
  fails to compile or panics; the criterion output is retained as a baseline artifact.
- **D2**: A `bench` job in `.github/workflows/rust.yml` invoking the 7 benches explicitly
  with trimmed criterion args + an `upload-artifact` step for `core-runtime/target/criterion`.
- **D3**: META_LEDGER entry (canonical markup); BACKLOG B-34 → done; B-06 folded (default-
  feature harness intent satisfied); B-34b (threshold gate) confirmed seeded.
- **D4**: The `bench` job appears in the PR-to-main CI run and concludes **success** (all 7
  benches compiled + ran). Locally: the 7 benches each run to completion with the trimmed args.

## CI Commands

```bash
cargo bench -p gg-core --bench ipc_throughput -- --warm-up-time 1 --measurement-time 2 --sample-size 10   # local smoke (repeat per CI-safe bench)
cargo fmt --check                                                                                          # formatting (YAML untouched by fmt)
```
