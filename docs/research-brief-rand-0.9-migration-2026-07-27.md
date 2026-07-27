# Research Brief

**Date**: 2026-07-27
**Analyst**: The Qor-logic Analyst
**Target**: `rand` 0.8 → 0.9 migration for `core-runtime` (Dependabot low-severity item, crypto-adjacent)
**Scope**: Every `rand`/`rand_core` API surface used by GG-CORE, with emphasis on the cryptographic RNG paths (nonce, salt, and key generation).

---

## Executive Summary

The `rand` 0.8 → 0.9 bump is **not** a pure lockfile bump: it contains one hard
breaking change on GG-CORE's security-critical path. In `rand_core` 0.9, `OsRng`
was demoted from the infallible `RngCore` trait to the **fallible `TryRngCore`
trait only**, so every `OsRng.fill_bytes(&mut buf)` call in the security module
fails to compile. The remaining changes are renames (`thread_rng`→`rng`,
`gen_range`→`random_range`) that still compile as `#[deprecated]` shims but would
break our `-D warnings` gate. The free function `rand::random()` (used for
nonce/salt generation) is unchanged. All findings below are verified against the
vendored crate source, not documentation.

## Findings

### Interface: `OsRng` — infallible `RngCore` removed (BREAKING)
- **Location**: `rand_core-0.9.3/src/os.rs:47,83,102` — `pub struct OsRng;`,
  `impl TryRngCore for OsRng`, `impl TryCryptoRng for OsRng`. There is **no**
  `impl RngCore for OsRng` in 0.9 (there was in 0.8).
- **Re-export unchanged**: `rand-0.9.2/src/rngs/mod.rs:110` → `pub use rand_core::OsRng;`
  so the path `rand::rngs::OsRng` still resolves.
- **0.8 behavior (current code)**: `use rand::RngCore; rand::rngs::OsRng.fill_bytes(buf)`
  — infallible, panics internally only on catastrophic OS failure.
- **0.9 replacement (verified adapter)**: `rand_core-0.9.3/src/lib.rs:232` provides
  `TryRngCore::unwrap_err(self) -> UnwrapErr<Self>`, and `:300` `impl<R: TryRngCore>
  RngCore for UnwrapErr<R>` with `:312 fn fill_bytes`. So
  `use rand::TryRngCore; rand::rngs::OsRng.unwrap_err().fill_bytes(buf)` restores the
  exact prior semantics (infallible call, panic on OS-entropy failure — the correct,
  auditable contract for cryptographic key material).
- **Verified Against Blueprint**: MATCH intent (CSPRNG preserved: `OsRng` remains the
  OS entropy source and `TryCryptoRng` marks it cryptographically secure). Mechanism
  DRIFT (trait moved) — remediated by the `unwrap_err()` adapter.

### Interface: `Rng::gen_range` / `thread_rng()` — renamed (deprecation)
- **Location**: `rand-0.9.2/src/rng.rs:333` `#[deprecated(since="0.9.0", note="Renamed
  to random_range")] fn gen_range`; `:161 fn random_range`. `rand-0.9.2/src/lib.rs:121`
  `#[deprecated(since="0.9.0", note="Renamed to rng")] pub fn thread_rng`; `:123 pub fn rng`.
- **Impact**: `bucket.rs:21` `rand::thread_rng().gen_range(0..100)` still compiles but
  emits two deprecation warnings → **fails `cargo clippy -- -D warnings`**.
- **Fix**: `rand::rng().random_range(0..100)` (keep `use rand::Rng;` — `random_range`
  is on the `Rng` trait, `rng.rs:161`).

### Interface: `rand::random()` free function — unchanged
- **Location**: `rand-0.9.2/src/lib.rs:172 pub fn random<T>()` (not deprecated).
- **Impact**: `encryption_core.rs:189`, `encryption_key.rs:89`,
  `encryption_tests.rs` (many) use `rand::random()` / `rand::random::<T>()` — compile
  unchanged. `StandardUniform` distribution rename is internal; our call sites don't
  name it.

### Dependency Chain
- **Direct**: `core-runtime/Cargo.toml:59 rand = "0.8"` — the only direct declaration.
- **Transitive 0.9 already present**: `Cargo.lock` already contains `rand 0.9.2`,
  `rand_chacha 0.9.0`, `rand_core 0.9.5`, `rand_distr 0.5.1` (pulled by `candle`),
  alongside the 0.8.5 tree our direct dep forces. Bumping our direct dep to `0.9`
  **collapses the duplicate rand tree**, removing 0.8.5/0.6.4/0.3.1 — a supply-chain
  and binary-size win, not just a CVE closure.

## Blueprint Alignment

| Blueprint Claim | Actual Finding | Status |
|---|---|---|
| `rand` used only for CSPRNG (Cargo.toml comment "Cryptographically secure RNG") | `OsRng` (OS entropy) + `rand::random()` (ThreadRng, ChaCha12 CSPRNG) — both cryptographically secure in 0.9 | MATCH |
| Section 4 Razor: edits stay within touched fns/files | All fixes are ≤1-line call-site swaps; no fn crosses 40 lines | MATCH |
| Offline / no-network constraint | `rand`/`rand_core`/`getrandom` are offline OS-entropy only; no new deps introduced | MATCH |

## Recommendations

1. **[P1]** Swap the 7 `OsRng.fill_bytes` sites to `use rand::TryRngCore;` +
   `OsRng.unwrap_err().fill_bytes(...)`. Crypto-critical — verify each in review.
2. **[P1]** `bucket.rs`: `thread_rng().gen_range` → `rng().random_range`.
3. **[P2]** Bump `Cargo.toml` `rand = "0.9"`; run `cargo update -p rand` and confirm
   the 0.8.x rand tree is fully removed from `Cargo.lock`.
4. **[P2]** Run the full feature matrix locally (`--features python,ffi,gguf,onnx`)
   since `OsRng` sites live in `security/` which every feature links.

## Updated Knowledge

New Shadow Genome entry: **"deprecation-shim ≠ trait-parity"** — a dependency major
bump can keep a symbol *callable* (deprecated shim) while silently moving the trait
that a *different* call site relies on; the compiler only surfaces the second when
the first is already fixed. Grep the whole trait surface, not just the renamed fn.

---

_Research complete. Findings are advisory — implementation decisions remain with the Governor._
