# Plan: B-35 — Profile the Default `Runtime::infer` Hot Path (Security-Overhead Bench)

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - This ships a **measurement** deliverable only: a `security_overhead` criterion bench that
    quantifies `SecurityPipeline::scan_prompt` + `sanitize_output` per-call latency (the tax
    every `Runtime::infer` pays, B-33), added to B-34's CI bench job, plus a hotspot ranking
    in the seal. It does NOT optimize any path — B-36..B-38 act on the evidence this produces.
- non_goals:
  - No change to `SecurityPipeline`, the sanitizer, or the injection filter logic.
  - No timing-regression threshold (that is B-34b); this bench joins the smoke+baseline set.
- exclusions:
  - No change to `test`/`clippy`/`fmt`/`features` jobs; no runtime source change.

## Open Questions

None. The pipeline is constructible without a model (`SecurityPipeline::from_config(
&SecurityConfig::default())`, verified against `security_pipeline_wiring_test.rs:17` and
`security/mod.rs:55` — `SecurityConfig::default()` enables both stages in blocking mode);
`gg_core::security::{SecurityConfig, SecurityPipeline}` is the public import path.

## Design Rationale (Simple Made Easy)

B-33 made `SecurityPipeline` mandatory on every inference. The optimization pass must measure
that mandatory cost before optimizing it — otherwise "secure by default" hides an
unquantified regression, and B-36's streaming-sanitizer decision is guesswork. The bench is a
pure value harness: construct one pipeline, iterate `scan_prompt`/`sanitize_output` over sized
inputs, report ns + throughput. No model, no GPU, default feature → it joins B-34's CI-safe
set unchanged. The per-call `sanitize_output` curve is the load-bearing output: it decides
whether B-36 (incremental streaming sanitize) is warranted.

## Phase 1: Add the `security_overhead` bench

### Affected Files

- `core-runtime/benches/security_overhead.rs` (NEW) — criterion bench, `harness = false`:
  - `blocking_pipeline() -> SecurityPipeline` = `SecurityPipeline::from_config(
    &SecurityConfig::default())` (both stages enabled, blocking — the default product config).
  - `bench_scan_prompt`: group `security_scan_prompt`, over clean prompts of
    `256`/`2048`/`16384` chars, `Throughput::Bytes`, `b.iter(|| pipeline.scan_prompt(p))`.
  - `bench_sanitize_output`: group `security_sanitize_output`, over clean outputs of
    `256`/`2048`/`16384` chars, `Throughput::Bytes`, `b.iter(|| pipeline.sanitize_output(o))`,
    PLUS one `pii_heavy_2048` case (a 2048-char string densely seeded with email/phone-shaped
    tokens) to measure the redaction-active cost vs the clean pass.
  - `criterion_group!` + `criterion_main!` per the `inference_latency.rs` convention.
- `core-runtime/Cargo.toml` — add `[[bench]]\nname = "security_overhead"\nharness = false`.
- `.github/workflows/rust.yml` — append `--bench security_overhead` to the `bench` job's
  `cargo bench` invocation (the CI-safe set becomes 7 benches; unchanged trimmed criterion
  args `-- --warm-up-time 1 --measurement-time 2 --sample-size 10`).

### Changes

New bench file + one `[[bench]]` stanza + one `--bench` flag. No runtime source change.

### Unit Tests

- No unit test (a benchmark IS the executable verification). The bench itself asserts the
  measured behavior: it constructs the real `SecurityPipeline` and invokes `scan_prompt` /
  `sanitize_output` on sized inputs — if either the construction path or a scan/sanitize call
  regressed to a panic or a signature break, the bench fails to compile or run (the same gate
  B-34 established). Local verification: `cargo bench --bench security_overhead -- --warm-up-time 1
  --measurement-time 2 --sample-size 10` runs to completion and prints per-size ns + throughput
  for both groups incl. the `pii_heavy` case.

## Feature Inventory Touches

Empty — justified. CI/measurement-infrastructure change (a benchmark); no user-touchable
runtime feature is introduced or modified (the `SecurityPipeline` surface is unchanged).

## Definition of Done

### Deliverable: `security_overhead` bench quantifying the per-call security tax

- **D1**: The mandatory per-call cost of `SecurityPipeline` (scan + sanitize) on the default
  `Runtime::infer` path is measured across input sizes, incl. a PII-active case, and ranked
  against the existing `inference_latency` validation anchor — producing B-36..B-38's target list.
- **D2**: `core-runtime/benches/security_overhead.rs` (NEW) with `bench_scan_prompt` +
  `bench_sanitize_output` groups constructing `SecurityPipeline::from_config(&SecurityConfig::
  default())`; `[[bench]] name = "security_overhead" harness = false` in `Cargo.toml`.
- **D3**: META_LEDGER entries (canonical markup) for research (#155, done), plan, audit, seal;
  BACKLOG B-35 → done; a hotspot-ranking paragraph in the seal entry.
- **D4**: `cargo bench --bench security_overhead -- --warm-up-time 1 --measurement-time 2
  --sample-size 10` runs to completion locally (both groups + `pii_heavy_2048` report ns +
  throughput); CI: the `bench` job (now incl. `--bench security_overhead`) concludes success on
  the PR-to-main run.

## CI Commands

- `cargo bench --bench security_overhead -- --warm-up-time 1 --measurement-time 2 --sample-size 10` — the new bench compiles + runs to completion
- `cargo fmt --check` — formatting (Rust bench file)
- `cargo clippy --benches -- -D warnings` — lint the new bench under default features
