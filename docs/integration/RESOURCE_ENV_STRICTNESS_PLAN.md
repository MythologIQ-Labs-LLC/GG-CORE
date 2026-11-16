# Resource environment parsing: strictness plan (#194)

**Status:** PROPOSED, NOT IMPLEMENTED. Risk L2. Requires the project's plan audit and a separate source/test PR before changing runtime behavior. Snapshot: 2026-11-16. No release/pin authority.

## Observed defect and security distinction

On current `main`, `core-runtime/src/config.rs` reads many explicitly configured `GG_CORE_*` values through `parse_usize/u32/u64` with `unwrap_or(default)` and then applies `max` or `clamp`. Thus invalid, zero, or out-of-range operator values can unexpectedly turn into a larger effective allowance, instead of failing or explicitly warning. Meanwhile `config_memory.rs::resolve` already strictly refuses invalid explicit memory ceilings. This is **configuration authority ambiguity**, not evidence of bypassing the explicit total-memory or Windows Job Object bound.

The `core-runtime/src/cli/config_cmd.rs::run_validate` path reads the *post-clamp* effective config; its `resource_max_concurrent == 0` warning is unreachable when `GG_CORE_MAX_CONCURRENT=0`. Existing tests actively expect invalid-value defaults and zero clamping, so tests must be updated deliberately, not disabled.

## Proposed per-variable resolution policy

| Variable(s) | Existing handling | Proposed explicit-value policy | Default when absent |
| --- | --- | --- | --- |
| `GG_CORE_MAX_CONCURRENT`, `GG_CORE_MAX_QUEUE_DEPTH`, `GG_CORE_MAX_CONTEXT_TOKENS`, `GG_CORE_BATCH_MAX_REQUESTS`, `GG_CORE_BATCH_MAX_TOKENS`, `GG_CORE_MAX_CONNECTIONS` | Bad text → default; zero → 1 | Strict decimal, representable, **>= 1**; refuse malformed, zero and overflow | Keep current values (2, 256, 4096, 8, 4096, 64) |
| `GG_CORE_MAX_CONTEXT` | Bad text → 4096; effective value clamped 1..1,000,000 | Strict decimal in **1..=1,000,000**; refuse out-of-range rather than silently substitute limit | 4096 |
| `GG_CORE_N_CTX` | Bad text → 2048; below 128 → 128 | Strict decimal `u32` **>= 128**; reject less or overflow; retain backend/context upper-bound validation rather than invent an upper limit here | 2048 |
| `GG_CORE_N_THREADS` | Bad text → 0 (auto), all parseable u32 accepted | Strict decimal `u32`, **0 is the documented auto sentinel**; nonzero remains explicit; preserve thread/worker policy and existing bounds | 0 (auto) |
| `GG_CORE_IPC_FRAME_LIMIT` | Bad text → 16 MiB; below 4096 → 4096 | Strict decimal `usize` **>= 4096**; refuse smaller or malformed; preserve any existing transport maximum | 16 MiB |
| `GG_CORE_STREAM_STALL_MS` | Bad text → 5000; below 100 → 100, partially warned | Strict decimal `u64` **>= 100**; refuse smaller or malformed; keep effective duration semantics | 5000 |
| `GG_CORE_SHUTDOWN_TIMEOUT`, `GG_CORE_SESSION_TIMEOUT` | Bad text → default; zero → 1 | Strict decimal `u64` **>= 1**; refuse malformed, zero and overflow; preserve existing duration semantics | 30 and 3600 seconds |
| `GG_CORE_MAX_TOTAL_MEMORY`, `GG_CORE_MAX_MEMORY_PER_CALL` | Already strict via `config_memory.rs` | **No behavior change.** Retain exact explicit memory authority, contradictions refused | Existing host-derived/optional defaults |

**Implementation discovery (#206, not yet qualified):** The GGUF native backend stores
resolved threads as `i32` (`engine/gguf/backend.rs::resolve_threads`).
Previously, `GG_CORE_N_THREADS` values above `i32::MAX` were accepted as `u32`
but silently changed to four threads by `i32::try_from(...).unwrap_or(4)`.
That contradicts the explicit-value requirement. For this variable only,
the strict implementation therefore accepts **0..=2,147,483,647** and refuses
larger values rather than pretending to apply them. Zero remains auto.
This is a source-level representability boundary, **not** a new performance
or worker-policy limit. It needs source/packaged qualification and independent
review before merging the plan correction or implementation.

**Out of scope:** changing `CORE_AUTH_TOKEN`, model admission and inference algorithms; adding arbitrary new max caps; changing legitimate omitted-variable defaults; redefining the auto-thread sentinel; touching frozen RC1 or downstream CoreForge pins.

## Smallest safe implementation route (separate branch)

1. Use one pure parser/resolver for present-vs-absent values, typed decimal parsing, boundary validation, and provenance of each resolved value. Reject invalid explicit UTF-8 or platform-incompatible values consistently, without logging the raw secret-bearing environment.
2. Extend `ConfigError` with a non-exhaustive typed resource-configuration error, keeping the existing memory error behavior and compatibility considerations. Make all `try_load` callers and standalone `load_gguf_config` follow the same effective semantics, avoiding a bypass through the GGUF-only helper.
3. Make `config validate`, `config show`, and `serve` use the same resolver. Report an explicit error for an invalid setting rather than displaying an unannounced default; effective output must distinguish operator-specified values from documented defaults when necessary for clarity.
4. Preserve valid settings and current admission/sandbox semantics. Update any existing positive tests that depended on silent clamping with explicit invalid-value refusal tests, plus unchanged-default and valid-override tests.
5. Run mandatory source qualification, config/CLI process-isolated negative tests and targeted packaged refusal checks before proposing merge. Check that invalid configuration never starts the serving daemon or creates an IPC listener, and that a valid config still preserves authentication, isolation and resource caps.

## Required adversarial negative tests

- Every listed variable: absent, valid boundary, invalid alphabetic, empty, whitespace/sign, overflow, zero (except `N_THREADS`), and non-UTF-8 where supported.
- `GG_CORE_MAX_CONTEXT=1000001`, `GG_CORE_N_CTX=127`, `GG_CORE_IPC_FRAME_LIMIT=4095`, `GG_CORE_STREAM_STALL_MS=99`: explicit refusal and no unexpected authority expansion.
- `GG_CORE_N_THREADS=0` still intentionally means auto, and valid explicit thread counts remain undivided in the embedder path.
- `config validate` and `serve` agree on the same variable/value. No unapproved clamp masquerades as valid configuration.
- A failed parse returns a predictable nonzero exit and a bounded variable-name/expected-range diagnostic (no prompts, tokens or unrelated values).
- The existing strict memory/Job Object tests still pass and valid combined config remains deterministic.

## Qualification and governance

The first deliverable is plan review and L2 audit of this decision table. Only then implement and test on a fresh source branch, without interfering with N7 #200 VM qualification. Review interface compatibility and source consumers before changing the `ConfigError` variants. Native Windows/packaged validation is required for any shipping claim. This document is a plan, **not proof of resolution**.
