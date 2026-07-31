# Plan: B-38 — Profile Memory + Fix `PromptCache::find_prefix` O(N²)→O(N)

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Adds a `prompt_cache_overhead` bench quantifying the `PromptCache` per-op cost, AND fixes
    `find_prefix` from O(N²) to O(N) hashing (one forward SHA256 pass). `MemoryPool` is untouched
    (a sound pool, already benched, no measured hotspot).
- non_goals:
  - No change to `MemoryPool`, `ResourceLimits`, or KV-cache code; no change to `PromptCache`
    public API, hashing scheme, or `get`/`insert`/`evict_lru` semantics.
  - No wiring of `PromptCache` into production (it stays dormant; this only removes its latent trap).
- exclusions:
  - No `test`/`clippy`/`fmt`/`features` job change beyond adding the new bench to the `bench` job.

## Open Questions

None. `find_prefix` semantics (longest cached prefix, `last_used` bump on hit) are preserved and
guarded by the existing `tests/prompt_cache_test.rs` (exact / partial / longest / no-match).
`sha2::Sha256` is `Clone`, enabling the O(N) forward pass.

## Design Rationale (Simple Made Easy)

`find_prefix` currently re-hashes `tokens[..len]` from scratch for every `len` N→1 → O(N²) SHA256.
Because SHA256 is a streaming hash and `Sha256` is `Clone`, all prefix hashes are available in ONE
forward pass: feed each token once, and at each step clone the hasher and finalize the clone to get
that prefix's hash. Tracking the longest prefix that hits the map as we scan short→long (the last
hit is the longest) makes it O(N) time and O(1) extra space, with byte-identical hashes and identical
longest-match semantics. The bench quantifies the before/after so the win is measured, not asserted.

## Phase 1: Add the `prompt_cache_overhead` bench

### Affected Files

- `core-runtime/benches/prompt_cache_overhead.rs` (NEW) — criterion bench, `harness = false`:
  - Helpers: build a `PromptCache` pre-populated with entries at several prefix lengths; a token
    vec of a given length.
  - `bench_hash_tokens`: `PromptCache::hash_tokens(&tokens)` over token counts {64, 512, 2048}
    (`Throughput::Elements`).
  - `bench_get`: `cache.get(&tokens)` (exact-match hit) over the same sizes.
  - `bench_find_prefix`: `cache.find_prefix(&tokens)` over token counts {64, 512, 2048} — the curve
    that exposes the O(N²)→O(N) change (this is the load-bearing measurement).
- `core-runtime/Cargo.toml` — add `[[bench]] name = "prompt_cache_overhead" harness = false`.
- `.github/workflows/rust.yml` — append `--bench prompt_cache_overhead` to the `bench` job (CI-safe
  set → 10). Model-free / default-feature.

### Unit Tests

No unit test (the bench is the executable verification); `find_prefix` correctness is covered by
Phase 2's existing tests. Local: `cargo bench --bench prompt_cache_overhead -- --warm-up-time 1
--measurement-time 2 --sample-size 10` runs to completion, printing the `find_prefix` size curve.

## Phase 2: `find_prefix` O(N²) → O(N)

### Affected Files

- `core-runtime/src/memory/prompt_cache.rs` — rewrite `find_prefix` to a single forward pass:
  ```rust
  pub fn find_prefix(&mut self, tokens: &[u32]) -> Option<(usize, CachedKv)> {
      let mut hasher = Sha256::new();
      let mut best: Option<(usize, [u8; 32])> = None;
      for (i, &t) in tokens.iter().enumerate() {
          hasher.update(t.to_le_bytes());
          let hash: [u8; 32] = hasher.clone().finalize().into();
          if self.entries.contains_key(&hash) {
              best = Some((i + 1, hash)); // short→long scan: last hit is the longest prefix
          }
      }
      let (len, hash) = best?;
      self.access_counter += 1;
      let counter = self.access_counter;
      let entry = self.entries.get_mut(&hash)?;
      entry.last_used = counter;
      Some((len, entry.clone()))
  }
  ```
  Same hashes as `hash_tokens(&tokens[..len])`, same longest-match result, same single `last_used`
  bump on a hit. `hash_tokens`/`get`/`insert`/`evict_lru` unchanged.

### Unit Tests

The existing `tests/prompt_cache_test.rs` is the regression floor and MUST pass unchanged:
`cache_find_prefix_exact`, `cache_find_prefix_partial`, `cache_find_prefix_longest_match`,
`cache_find_prefix_no_match` — they assert the returned `(prefix_len, entry)` for populated caches,
so an incorrect rewrite fails them.

## Feature Inventory Touches

- `entry_id`: `F-60` (prompt KV cache — exported but previously unindexed) — `operation`: `NEW`
  (first FEATURE_INDEX row for `prompt_cache.rs`) — `test_path`:
  `core-runtime/tests/prompt_cache_test.rs`. B-38 optimizes `find_prefix`; the index row + existing
  tests make it verifiable.

## Definition of Done

### Deliverable: measured PromptCache + O(N) `find_prefix`

- **D1**: `PromptCache` per-op cost is measured (incl. `find_prefix` across token counts), and
  `find_prefix` is O(N) not O(N²), with identical longest-prefix results.
- **D2**: NEW `benches/prompt_cache_overhead.rs` (+ `Cargo.toml` `[[bench]]` + `rust.yml` `--bench`);
  `find_prefix` rewritten to the single forward pass in `prompt_cache.rs`.
- **D3**: META_LEDGER entries (canonical markup) research #171, plan, audit, seal; BACKLOG B-38 →
  done; CHANGELOG note; FEATURE_INDEX F-60 added.
- **D4**: `cargo test -p gg-core --test prompt_cache_test` — the 4 `find_prefix` tests pass with the
  rewrite; `cargo bench --bench prompt_cache_overhead -- --warm-up-time 1 --measurement-time 2
  --sample-size 10` runs to completion (the `find_prefix` curve is now ~linear). Locally on the
  Windows dev host (default feature); CI-confirmed by the `test` + `bench` jobs.

## CI Commands

- `cargo test -p gg-core --test prompt_cache_test` — the find_prefix rewrite passes the existing tests
- `cargo bench --bench prompt_cache_overhead -- --warm-up-time 1 --measurement-time 2 --sample-size 10` — the new bench runs to completion
- `cargo fmt --check` — formatting
- `cargo clippy -p gg-core --benches -- -D warnings` — lint
