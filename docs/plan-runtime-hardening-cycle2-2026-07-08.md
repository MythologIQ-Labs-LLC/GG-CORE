# Plan: Runtime Hardening Cycle 2 (rev.3) — validate_path + clippy residuals (#55–#57)

**change_class**: feature
**doc_tier**: standard
**high_risk_target**: false (validation hardening + lint hygiene in a compute-only runtime)

**Session**: 2026-07-08T1651-6c68b6 (research gate sealed; audit VETO'd rev.1 at ledger Entry #82)

**Revision note**: rev.1 was VETO'd (Entry #82). #58 (KV cross-sequence isolation)
is **removed from this cycle** — the audit proved `PageTable` is architecturally
single-sequence (one global position-keyed `entries` map, `src/memory/paged.rs:94-104`),
so it is a multi-tenant redesign (exclusive per-sequence page ownership or
`(SequenceId, block)` keying + eviction/remanence handling), not a mechanical fix.
Escalated to backlog B-20 for a dedicated L3 ideation/design cycle. This rev
keeps only the judged-sound remainder, with the judge's R3/R4 corrections applied.

**boundaries**:
- limitations: verification on the local integration preview (PR #47 base + cycle-1 + this commit); live CI needs operator push.
- non_goals: #58 KV isolation redesign (B-20), PR #47 merge, fmt-sweep integration, bench work.
- exclusions: no changes under `ipc/` or `sandbox/`; no `security/` **behavior** changes (the two `security/` clippy edits are a compile-time-assert rework + a no-op cast removal, both behavior-preserving and flagged for audit attention); no public API signature changes.

## Open Questions

None blocking.

## Phase 1: validate_path hardening + contract reconciliation (#57, #55)

### Affected Files
- `core-runtime/src/models/loader.rs` — NUL rejection + unit test case (test with file)
- `core-runtime/tests/security_path_traversal_test.rs` — unchanged; failing oracle `reject_null_byte_in_path` flips green
- `core-runtime/tests/integration_gguf_test.rs` — amend 2 tests to assert existence at the load step

### Changes
1. `loader.rs` `validate_path` (:75): early reject before path join —
   `if relative_path.contains('\0') { return Err(LoadError::PathNotAllowed(PathBuf::from("<nul-byte rejected>"))); }`.
   The error payload is a fixed sentinel, never the raw path (it contains NUL). Mirrors the existing k8s precedent (`src/k8s/types.rs:167`).
   Caller-safety confirmed: the three callers — `src/ffi/models.rs:34`, `src/python/session.rs:105`, internal — pass model-path strings that legitimately never contain NUL, so rejection changes no valid flow.
2. `loader.rs` unit test `test_validate_path` (:193): add `assert!(loader.validate_path("models/test\0../../etc/passwd").is_err());`. Existing lexical Ok/Err cases unchanged (contract preserved).
3. `integration_gguf_test.rs::gguf_model_requires_valid_path` (:170): assert `validate_path("models/nonexistent.gguf")` is **Ok** (lexical contract), then `load_metadata(&validated)` is `Err` (existence lives there, `loader.rs:105-107`, `LoadError::NotFound`).
4. `integration_gguf_test.rs::mmap_load_missing_file` (:176): assert `validate_path(...)` is Ok, then the mmap load entry point (`load_mapped` → `MappedModel::open`, `loader.rs:146-151`) is `Err` — the missing file surfaces as `LoadError::Io(_ /* NotFound */)`, so assert `.is_err()` (do NOT pin the `NotFound` variant — the mmap path returns the `Io` variant, per audit R3).

### Unit Tests
- `security_path_traversal_test.rs` — 9/9 green (NUL now rejected at the seam)
- `integration_gguf_test.rs` — 14/14 green (existence asserted at the load step, honest contract move: assertion relocated, not removed)
- `loader.rs::test_validate_path` — extended with the NUL case; lexical cases unchanged

## Phase 2: Residual clippy cleanup (#56)

**rev.3 note**: the map below is derived from captured `cargo clippy --all-targets
-- -D warnings` output on the preview base (verbatim lint name paired with its
`-->` location), not inferred from file names. Per Shadow Genome Entry #3.

### Ground-truth site map (13 errors)

| # | Site | Emitted lint | Fix |
|---|------|--------------|-----|
| 1 | `src/scheduler/thread_pool_tests.rs:3` | unused import (`super::super::thread_pool_types::*`) | delete the import |
| 2 | `src/models/lifecycle_tests.rs:19` | methods called `new` usually return `Self` (`new_ret_no_self`) | `#[allow(clippy::new_ret_no_self)]` on the mock ctor (intentionally returns `Arc<dyn GgufModel>`) |
| 3 | `src/models/persistence.rs:196` | field assignment outside of initializer for a `Default::default()` instance | fold into a single struct literal |
| 4 | `src/models/persistence.rs:274` | field assignment outside of initializer | fold into a single struct literal |
| 5 | `src/models/persistence.rs:336` | field assignment outside of initializer | fold into a single struct literal |
| 6 | `src/models/persistence.rs:341` | field assignment outside of initializer | fold into a single struct literal |
| 7 | `src/security/prompt_injection.rs:189` | casting to the same type is unnecessary (`u8` -> `u8`) | drop `as u8` in `severity as u8 * 5` (live `scan()`; `classify_severity` already returns `u8`, verified :235) — behavior-preserving |
| 8 | `src/ab_testing/metrics/stats.rs:61` | manual checked division | apply clippy suggestion (`checked_div`) for `total_latency_us / successes` inside the `if successes > 0` guard |
| 9 | `src/ab_testing/metrics/stats.rs:66` | manual checked division | apply clippy suggestion (`checked_div`) for `total_tokens / successes` inside the `if successes > 0` guard |
| 10 | `src/security/encryption_tests.rs:371` | this assertion has a constant value | **oracle rework, not delete**: `const _: () = assert!(ModelEncryption::PBKDF2_ITERATIONS >= 600_000);` (preserves OWASP PBKDF2 guarantee at compile time) |
| 11 | `src/cli/health.rs:94` | this assertion has a constant value | **oracle rework, not delete**: `const _: () = assert!(EXIT_HEALTHY == 0);` (preserves exit-code convention guarantee) |
| 12 | `src/cli/health.rs:95` | this assertion has a constant value | **oracle rework, not delete**: `const _: () = assert!(EXIT_UNHEALTHY != 0);` |
| 13 | `src/cli/ipc_client.rs:404` | this can be `std::io::Error::other(_)` | `std::io::Error::new(ErrorKind::Other, "test")` → `std::io::Error::other("test")` |

### Changes
Apply each row's fix. The three constant-value assertions (#10–12) are
security/convention **oracles** — reworked to `const _: () = assert!(...)` so the
guarantee is preserved (strengthened to compile-time), never deleted. All other
sites take clippy's own machine suggestion. No behavior change at any site.

Note: sites #10–12 relocate a test-body `assert!` to a module-level `const _`
item — verify at implement time that each referenced symbol (`PBKDF2_ITERATIONS`,
`EXIT_HEALTHY`, `EXIT_UNHEALTHY`) is `const` and in scope at module level (they
are `pub const`), and drop the now-empty `#[test] fn` wrappers if they contained
only the relocated assertion.

### Unit Tests
- No new tests. Gate: `cargo clippy --all-targets -- -D warnings` exit 0 on the preview base (Windows leg); the PBKDF2 + exit-code guarantees are now compile-time enforced (strictly stronger than the runtime `assert!`).

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|----------|-----------|-----------|-----------------|
| F-24 | MODIFIED | core-runtime/tests/integration_gguf_test.rs | load step errs for nonexistent model; validate_path stays lexical |
| F-33 | MODIFIED | core-runtime/tests/security_path_traversal_test.rs | NUL-byte path rejected at validation seam |
| F-35 | n/a-justified | core-runtime/src/security/encryption_tests.rs | PBKDF2>=600k oracle strengthened to compile-time assert; behavior unchanged |

## Definition of Done

### Deliverable: validate_path NUL rejection (#57) + contract fix (#55)
- **D1**: NUL bytes rejected at the validation seam; existence asserted where it lives.
- **D2**: early-reject in `validate_path` (sentinel error); 2 integration tests amended (mmap asserts `is_err`, not a pinned variant); 1 unit case added.
- **D3**: BACKLOG B-17/B-19 → in-progress with commit ref; FEATURE_INDEX F-24/F-33 rows updated same commit.
- **D4**: `cargo test --workspace --no-fail-fast` on preview — security_path_traversal_test 9/9, integration_gguf_test 14/14.

### Deliverable: clippy residuals (#56)
- **D1**: clippy gate viable on current stable toolchain; PBKDF2 guarantee preserved (strengthened to compile-time).
- **D2**: 13 fixes across 8 files; `encryption_tests.rs:371` reworked to `const _` assert, not deleted.
- **D3**: BACKLOG B-18 → in-progress.
- **D4**: `cargo clippy --all-targets -- -D warnings` exit 0 on preview (Windows leg; unix legs via CI post-push).

## CI Commands

- `cargo clippy --all-targets -- -D warnings` — lint gate
- `cargo test --workspace --no-fail-fast` — full suite; target: kv_cache still 13/14 (B-20 pending), gguf 14/14, path-traversal 9/9
- `cargo fmt --check` — touched files stay fmt-clean
