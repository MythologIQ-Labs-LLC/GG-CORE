# Plan — rand 0.8 → 0.9 Migration

**Date**: 2026-07-27
**Author**: Strategist
**Session ID**: 2026-07-27T-rand09
**Risk Grade**: L3 (cryptographic RNG in the security module)
**Source research**: `docs/research-brief-rand-0.9-migration-2026-07-27.md` (Entry #113)

---

## Objective

Bump the direct `rand` dependency 0.8 → 0.9 in `core-runtime`, remediating the one
breaking crypto-path change (`OsRng` no longer impls infallible `RngCore`) and the
two deprecation renames, without altering cryptographic behavior or introducing any
new dependency. Success = full feature matrix compiles under `-D warnings` and all
tests pass, with the duplicate rand 0.8.x tree removed from `Cargo.lock`.

## High-Risk Target / Impact Assessment (L3)

- **Affected security property**: confidentiality/integrity of key material — the
  changed call sites generate AES-GCM nonces, key-rotation keys, KDF salts, and IPC
  session tokens.
- **Failure mode if done wrong**: (a) using a non-CSPRNG source, (b) silently
  swallowing an OS-entropy failure and proceeding with zeroed/partial buffers.
- **Mitigation**: use only the crate-blessed `TryRngCore::unwrap_err()` adapter,
  which is byte-for-byte the prior semantics (OS entropy; panic — never continue —
  on entropy failure). No call site changes *which* RNG is used or *how many* bytes
  are drawn. Adversarial review (devil's advocate) explicitly checks each of the 7
  sites for entropy-source and length parity.

## Scope (exact files)

1. `core-runtime/Cargo.toml` — `rand = "0.8"` → `rand = "0.9"`.
2. `core-runtime/Cargo.lock` — regenerated via `cargo update -p rand`; verify the
   0.8.5 rand tree is gone.
3. `core-runtime/src/ab_testing/traffic/bucket.rs` — `thread_rng().gen_range(0..100)`
   → `rng().random_range(0..100)` (keep `use rand::Rng;`).
4. `core-runtime/src/ipc/auth_session.rs` — `use rand::RngCore;` → `use rand::TryRngCore;`;
   `OsRng.fill_bytes(buf)` → `OsRng.unwrap_err().fill_bytes(buf)`.
5. `core-runtime/src/security/audit_types.rs` — same OsRng swap.
6. `core-runtime/src/security/key_rotation.rs` — same OsRng swap.
7. `core-runtime/src/security/encryption_tests.rs` — same OsRng swap (test module).
8. `core-runtime/src/security/fips_tests.rs` — same OsRng swap ×3 (test module).

**Out of scope**: `encryption_core.rs`, `encryption_key.rs` (use `rand::random()`,
unchanged); any refactor beyond the call-site swaps; `rand_distr`/`candle` transitive
usage (already on 0.9 via candle).

## Definition of Done

- [ ] `cargo build -p gg-core --all-features` compiles.
- [ ] `cargo clippy -p gg-core --all-targets --all-features -- -D warnings` → 0 warnings
      (no residual `deprecated` on `rand`).
- [ ] `cargo test -p gg-core` (default) and per-feature test legs pass.
- [ ] `cargo fmt --check` clean.
- [ ] `Cargo.lock` contains exactly one `rand` (0.9.x); no `rand 0.8`, `rand_core 0.6`,
      `rand_chacha 0.3`.
- [ ] Every OsRng site still draws from OS entropy and fills the same byte count
      (adversarial review sign-off).
- [ ] CI green on all 11 legs (verified post-PR).

## Razor Compliance

All edits are ≤1-line call-site swaps or single-token renames; no function grows,
no file crosses 250 lines, nesting unchanged.

## Sequence

1. Edit the 6 source files + Cargo.toml.
2. `cargo update -p rand --precise 0.9.2` (or latest 0.9.x); inspect lock diff.
3. Local verify (build/clippy/test/fmt across features).
4. Adversarial crypto review of the 7 OsRng sites.
5. Substantiate seal → commit → PR → CI.

## Rollback

Single-commit revert; no data migration, no schema, no persisted state touched.
