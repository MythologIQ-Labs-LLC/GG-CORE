# Research Brief — B-39: Fix the Rotted `ipc_throughput` Bench

**Date**: 2026-07-30
**Analyst**: The Qor-logic Analyst
**Target**: B-39 — repair the `ipc_throughput` criterion bench (fixture schema drift found by
B-34's bench gate) and re-admit it to the CI bench set.
**Scope**: `core-runtime/benches/ipc_throughput.rs`, its `fixtures/prompts/*.json`, the CI
`bench` job's `--bench` list.

---

## Executive Summary

`ipc_throughput` panics because `fixture_to_request` reads `fixture["prompt"].as_str()`, but the
three `fixtures/prompts/*.json` carry `prompt_tokens` (an int array), not a top-level `prompt`
string. The fixtures are consumed ONLY by this bench, and `InferenceRequest.prompt` is a `String`,
so the clean fix is to derive the prompt string from the `prompt_tokens` array in the bench (one
source of size truth), then re-add `--bench ipc_throughput` to the CI `bench` job. No fixture data
change, no runtime change.

## Findings (verified)

### F1 — the exact panic
`fixture_to_request` (`ipc_throughput.rs:20-23`): `fixture["prompt"].as_str().expect("prompt must
be a string")`. The fixtures have no `prompt` key → `as_str()` yields `None` → the `.expect`
panics. B-34's `bench` job caught this (a compile/panic fails the job), which is why it was
excluded there with a comment (`rust.yml:106-107`) and filed as B-39.

### F2 — fixtures carry `prompt_tokens`, only this bench reads them
The three fixtures (`small`/`medium`/`large`.json) have `model_id`, `prompt_tokens` (100 / 1000 /
4000 ints — the size ladder), and `parameters {max_tokens, temperature}`. A repo grep shows NO
other `.rs` reads `fixtures/prompts/` (the `prompt_tokens` hits in `engine/speculative*.rs` are
unrelated function parameters). So the fixture schema is owned by this bench alone; changing how
the bench consumes it is safe.

### F3 — `InferenceRequest.prompt` is a `String`
`protocol_types.rs:72,75`: `pub struct InferenceRequest { … pub prompt: String, … }`. The bench
must supply a `String`. Deriving it from `prompt_tokens` (e.g. one placeholder word per token)
preserves the small/medium/large payload-size ladder the throughput bench depends on, and keeps a
single source of size truth (the token array) rather than duplicating a `prompt` string into each
fixture.

### F4 — re-admission to CI
`rust.yml:105-113`: the `bench` job runs 7 CI-safe benches; `ipc_throughput` is the excluded 8th.
Fixing F1 lets it rejoin: add `--bench ipc_throughput` and drop the exclusion comment. It is
model-free / GPU-free / default-feature (it builds `InferenceRequest` + exercises IPC protocol
encoding), so it is CI-safe like the others.

## Recommendations

1. **B-39 deliverable**: in `fixture_to_request`, replace the `fixture["prompt"]` read with a
   prompt derived from `fixture["prompt_tokens"]` (array length → a proportionally-sized `String`);
   keep `model_id`/`parameters` reads unchanged. Add `--bench ipc_throughput` back to the CI `bench`
   job and remove the B-39 exclusion comment.
2. **Verify locally** (default feature, Windows-buildable): `cargo bench --bench ipc_throughput --
   --warm-up-time 1 --measurement-time 2 --sample-size 10` runs all six bench functions to
   completion over small/medium/large without panic.

## Updated Knowledge (Shadow Genome)

Reinforces the B-34 finding: a bench that nothing runs silently rots when the data it reads drifts.
The CI `bench` gate is what surfaced this; B-39 closes the loop by repairing the reader and
re-admitting the bench, so the size-ladder IPC-encoding throughput is measured again.

---

_Research complete. B-39 = derive the prompt from `prompt_tokens` in the bench + re-add it to the
CI bench set. Fixture-and-bench-local; no runtime or fixture-data change._
