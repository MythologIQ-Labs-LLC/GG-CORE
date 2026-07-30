# Plan: B-39 — Fix the Rotted `ipc_throughput` Bench

**change_class**: hotfix

**doc_tier**: minimal

## Open Questions

None. The fixtures are bench-only (research F2), `InferenceRequest.prompt` is a `String` (F3), so
the bench derives the prompt from `prompt_tokens`; no fixture-data or runtime change.

## Phase 1: Derive the prompt from `prompt_tokens` + re-admit to CI

### Affected Files

- `core-runtime/benches/ipc_throughput.rs` — in `fixture_to_request`, replace the panicking
  `fixture["prompt"].as_str().expect("prompt must be a string")` read with a prompt derived from
  the `prompt_tokens` array: one placeholder word per token, space-joined, so the small/medium/
  large (100/1000/4000) payload-size ladder is preserved. `model_id` and `parameters` reads
  unchanged.
  ```rust
  let token_count = fixture["prompt_tokens"]
      .as_array()
      .expect("prompt_tokens must be an array")
      .len();
  let prompt = vec!["word"; token_count].join(" ");
  ```
- `.github/workflows/rust.yml` — add `--bench ipc_throughput` back to the `bench` job's
  `cargo bench` list and remove the B-39 exclusion comment (`:106-107`). The CI-safe set becomes 8.

### Changes

Bench reader + one CI `--bench` flag. No fixture JSON change, no runtime source change.

### Unit Tests

No unit test (a benchmark is the executable verification). Local verification: `cargo bench
--bench ipc_throughput -- --warm-up-time 1 --measurement-time 2 --sample-size 10` runs all six
bench functions to completion over small/medium/large fixtures without panic (the exact failure
B-34 caught). The derived prompt is non-empty (token_count ≥ 100), satisfying
`InferenceRequest::validate`'s non-empty-prompt check.

## Feature Inventory Touches

Empty — justified. Test/measurement-infrastructure repair (a benchmark reader); no user-touchable
runtime feature is introduced or modified.

## Definition of Done

### Deliverable: `ipc_throughput` runs green and rejoins the CI bench set

- **D1**: The IPC-encoding throughput bench runs over the small/medium/large size ladder without
  panicking, and is exercised by the CI `bench` job on every PR to `main`.
- **D2**: `fixture_to_request` derives `prompt` from `prompt_tokens`; `rust.yml` `bench` job lists
  `--bench ipc_throughput` and the exclusion comment is gone.
- **D3**: META_LEDGER entries (canonical markup) research #165, plan, audit, seal; BACKLOG B-39 →
  done.
- **D4**: `cargo bench --bench ipc_throughput -- --warm-up-time 1 --measurement-time 2
  --sample-size 10` runs to completion locally (all six functions, exit 0); CI: the `bench` job
  (now incl. `--bench ipc_throughput`) concludes success on the PR-to-main run.

## CI Commands

- `cargo bench --bench ipc_throughput -- --warm-up-time 1 --measurement-time 2 --sample-size 10` — the repaired bench runs to completion
- `cargo fmt --check` — formatting
- `cargo clippy --bench ipc_throughput -- -D warnings` — lint the bench
