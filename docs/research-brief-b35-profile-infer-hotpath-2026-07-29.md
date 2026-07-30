# Research Brief — B-35: Profile the Default `Runtime::infer` Hot Path

**Date**: 2026-07-29
**Analyst**: The Qor-logic Analyst
**Target**: B-35 — quantify the per-call `SecurityPipeline` overhead (the tax every
`Runtime::infer` now pays, B-33) and rank the default-path hotspots, so B-36..B-38 optimize
against evidence. Second cycle of the optimization pass; builds on B-34's CI bench baseline.
**Scope**: the `SecurityPipeline` scan/sanitize cost + a benchable construction path.

---

## Executive Summary

`Runtime::infer` runs `scan_prompt` (regex injection detection) + `sanitize_output` (regex
PII redaction) on every call — cost currently unmeasured. `SecurityPipeline::from_config`
constructs the pipeline with no model (verified via `security_pipeline_wiring_test`), so a
`security_overhead` criterion bench is straightforward and CI-safe. B-35's deliverable is
that bench (added to B-34's CI bench job) + a profiling analysis that ranks the default hot
path against the existing `inference_latency` (validation) numbers — producing the
evidence-backed target list for B-36 (streaming sanitizer) and beyond.

## Findings (verified)

### F1 — the security tax is real and unmeasured
- `Runtime::infer` (`runtime_facade.rs:66,77`): `scan_prompt(prompt)` then `sanitize_output(
  &result.output)`. `scan_prompt` (`pipeline.rs:84`) runs `PromptInjectionFilter` (regex
  matches + severity scoring); `sanitize_output` (`:113`) runs the PII sanitizer (regex
  redaction). Both are per-call, input-size-dependent regex scans — the exact cost the
  optimization pass must quantify. No bench measures it today.

### F2 — the pipeline is benchable without a model
- `SecurityPipeline::from_config(&SecurityConfig { enable_prompt_injection_detection: true,
  block_prompt_injection: true, enable_pii_detection: true, redact_pii: true, .. })` (per
  `security_pipeline_wiring_test.rs:17 blocking_pipeline()`). `SecurityConfig` +
  `SecurityPipeline` are public (`gg_core::security::{SecurityConfig, SecurityPipeline}`).
  `scan_prompt(&str) -> ScanVerdict`, `sanitize_output(&str) -> SanitizedOutput` — a
  criterion bench constructs one pipeline and iterates scan/sanitize over sized inputs. No
  model, no GPU, default feature → CI-safe (joins B-34's bench set).

### F3 — the cost driver is regex over input size
- Both paths scan the input with `\b`-anchored regex pattern sets. Cost scales with input
  length × pattern count. Benching at small/medium/large prompt + output sizes isolates the
  per-byte tax and reveals whether it is material vs the model inference time (llama.cpp
  generation dominates for real models, but for short prompts / embedding/classify calls the
  security tax is a larger relative fraction).

### F4 — the streaming sanitizer is the suspected superlinear case (feeds B-36)
- The B-24b streaming egress sanitizer re-sanitizes the full accumulated buffer per push. A
  `sanitize_output` bench over increasing output sizes will show the per-call scaling; if it
  is linear per call, the streaming re-sanitize is O(n²) over n pushes — the B-36 trigger.
  B-35 provides the per-call curve that decides B-36.

### F5 — validation/param cost is already benched (comparison anchor)
- `inference_latency` benches `input_validation` / `params_creation` (~3.6 ns) — a yardstick:
  if `scan_prompt` is ~1000× that, the security tax dominates the non-model default path.

## Recommendations

1. **B-35 deliverable**: add `core-runtime/benches/security_overhead.rs` — a criterion bench
   that constructs a blocking `SecurityPipeline` and measures `scan_prompt` (over
   small/medium/large clean prompts) + `sanitize_output` (over small/medium/large outputs,
   incl. one PII-heavy case). Add it to B-34's CI `bench` job. Output: the quantified
   per-call + per-byte security tax.
2. **Profiling analysis** (in the seal / a short note): rank the default hot path —
   scan/sanitize vs validation vs tokenize (existing benches) — with the measured numbers, to
   set B-36..B-38 targets. Do NOT optimize in this cycle (B-35 measures; B-36+ act).
3. **Sequencing**: the `sanitize_output` size curve directly decides B-36 (streaming
   sanitizer incrementalization). B-34b (threshold gate) still queued (needs more baseline
   runs).

## Updated Knowledge (Shadow Genome)

Discipline: **profile the tax you added.** B-33 made the `SecurityPipeline` mandatory on
every inference; the same initiative must measure that mandatory cost, or "secure by
default" hides an unquantified perf regression. Measure the wrapper's overhead as part of
shipping the wrapper.

---

_Research complete. B-35 = a `security_overhead` bench + hotspot ranking; feeds B-36. The
per-call sanitize curve is the load-bearing measurement._
