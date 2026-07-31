# Research Brief — B-38: Profile Memory Overhead (pool / prompt-cache)

**Date**: 2026-07-30
**Analyst**: The Qor-logic Analyst
**Target**: B-38 — memory-overhead reduction guided by the `memory_overhead` bench, covering the
`MemoryPool` and the `PromptCache`. Fifth cycle of the optimization pass; measurement-first.
**Scope**: `memory/pool.rs`, `memory/prompt_cache.rs`, and the `memory_overhead` bench.

---

## Executive Summary

Two distinct findings. (1) The production memory hot path — `MemoryPool` — is a genuine
`parking_lot::Mutex<VecDeque<Vec<u8>>>` buffer pool (real reuse, cheap lock) and is already benched
by `memory_overhead`; no evidence of a hotspot there. (2) `PromptCache` is exported but **wired into
nothing in production** (no `PromptCache::new` outside tests), and its `find_prefix` is **O(N²)** in
SHA256 work: it re-hashes each prefix `tokens[..len]` from scratch for every `len` from N down to 1.
This is a real superlinear inefficiency of the same shape B-36 fixed — but it is currently
**dormant** (nothing pays for it). `PromptCache` is unbenched. Recommendation: bench the PromptCache
to quantify the O(N²) `find_prefix` (and `get`/`insert`/`hash_tokens`); the decision of whether to
also *fix* `find_prefix` now (a clean, test-covered O(N) incremental-hash change) vs flag it for when
the cache is wired is a scope call for the operator, since optimizing dormant code is a YAGNI
judgment.

## Findings (verified)

### F1 — `MemoryPool` is a real pool, already benched
- `pool.rs:60-79`: `MemoryPool { buffers: Arc<Mutex<VecDeque<Vec<u8>>>> }` (parking_lot Mutex);
  `acquire` pops a buffer or allocates; `PooledBuffer` on drop pushes back (`:54`). Genuine reuse
  with a fast synchronous lock. `benches/memory_overhead.rs` already covers `acquire` + the
  `ResourceLimits` path. No unbenched production memory hot path here.

### F2 — `PromptCache::find_prefix` is O(N²) in hashing
- `prompt_cache.rs:85-98`: `for len in (1..=tokens.len()).rev() { let hash =
  hash_tokens(&tokens[..len]); … }`. `hash_tokens` (`:45-51`) is a full SHA256 over `len` tokens.
  Total work = Σ_{len=1}^{N} O(len) = **O(N²)** SHA256 byte-hashing per `find_prefix` call, for an
  N-token prompt — the longest-prefix scan re-hashes the whole prefix at every length. `get` (`:54`)
  is a single O(N) hash (fine); `insert`/`evict_lru` are O(1)/O(entries).

### F3 — `PromptCache` is DORMANT (no production caller)
- `memory/mod.rs:32` exports `PromptCache`, but a repo grep finds NO `PromptCache::new` /
  `find_prefix` call in `src/` outside `prompt_cache.rs` itself; the only callers are
  `tests/prompt_cache_test.rs`. So the O(N²) `find_prefix` costs nothing today — it is a latent trap
  that would bite when the cache is wired for KV prefix reuse. `PromptCache` is unbenched.

### F4 — the O(N) fix is clean and test-covered (if in scope)
- `sha2::Sha256` implements `Clone`, so all prefix hashes can be produced in ONE forward pass: feed
  tokens `1..N`, and at each step clone the hasher and finalize the clone to get the prefix hash at
  that length — O(N) total hashing. Collect the (len, hash) pairs and check `contains_key`
  longest-first. Behavior is identical (same hashes, same longest-match semantics); `tests/
  prompt_cache_test.rs` already asserts `find_prefix` exact/partial/longest/no-match, so it is a
  guarded change. This is the B-36-shaped fix, but for a dormant component.

## Blueprint Alignment

| Optimization-brief expectation | Finding | Status |
|---|---|---|
| Memory-overhead reduction (pool) | MemoryPool is a real pool, already benched (F1) | MATCH — no pool hotspot |
| Memory-overhead reduction (prompt-cache) | PromptCache unbenched; find_prefix O(N²) but dormant (F2/F3) | DRIFT → bench it; fix is a scope call |

## Recommendations

1. **B-38 deliverable (measurement, always)**: add `benches/prompt_cache_overhead.rs` measuring
   `hash_tokens` / `get` / `insert` / `find_prefix` across token counts, quantifying the O(N²)
   `find_prefix` curve. Join the CI bench job.
2. **Scope fork (operator)**: either (A) ALSO fix `find_prefix` to O(N) now (F4 — clean,
   test-covered, prevents shipping O(N²) when the cache is wired), the bench proving before/after; or
   (B) measure + flag only, queuing the fix as B-38b for when `PromptCache` gains a production caller
   (pure measure-first / YAGNI, since nothing pays the cost today).
3. **Do NOT touch `MemoryPool`** — it is a sound pool with no measured hotspot.

## Updated Knowledge (Shadow Genome)

**Measure dormant components before optimizing them.** `find_prefix` is genuinely O(N²), but it has
no production caller — fixing it now is a YAGNI judgment, not an obvious win. The measurement
(the bench) is always worth it (it documents the latent trap so wiring the cache later isn't blind);
the fix is conditional on whether the component is (or is about to be) live.

---

_Research complete. B-38 = a `prompt_cache_overhead` bench quantifying the O(N²) `find_prefix`;
whether to also ship the O(N) fix for the dormant cache is a scope decision. MemoryPool is fine._
