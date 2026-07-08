# Execution Plan: B-20 — KV Cache Cross-Sequence Isolation Redesign (issue #58)

> **Audience**: an implementer (possibly a smaller model) executing against a
> fixed spec. Every path, signature, and command is explicit. Do not improvise
> the design — it was audit-gated. If reality diverges from a cited line, STOP
> and report rather than guessing.

**Risk grade**: L3 (multi-tenant memory isolation). This is a security fix.
**Canonical issue**: GitHub #58. **Backlog**: B-20.
**Origin**: escalated out of hardening cycle 2 by audit VETO (META_LEDGER Entry #82)
because the naive "consult page_ids" fix was defeated by the PageTable's global
position-keyed allocation. This plan fixes the architecture, not the symptom.

Governance chain to follow: this plan is a fresh initiative → run
`/qor-ideate` (optional) → `/qor-plan` (may reuse this doc as the plan body) →
`/qor-audit` (MANDATORY PASS, L3) → `/qor-implement` → `/qor-substantiate`.
Do NOT implement before an audit PASS.

---

## 1. The defect (verified against source, 2026-07-08)

`core-runtime/src/memory/paged.rs` `PageTable` keys physical pages by a **single
global position index**, shared across all sequences:

- `entries: Vec<Option<PageId>>` (`paged.rs:72`) indexed by `page_idx = seq_pos / PAGE_TOKENS`.
- `allocate(seq_pos)` (`paged.rs:94-105`) dedups on `entries[page_idx]`: if any
  sequence already allocated at that global index, the SAME `PageId` is returned
  (`paged.rs:98-100`). Two sequences at position 0 therefore share one page.
- `get(seq_pos)` / `get_mut(seq_pos)` (`paged.rs:124-137`) resolve by the same
  global `page_idx` — reads and writes collide across sequences.

Per-sequence page ownership **is** tracked (`entry.page_ids` in
`SequenceEntry`, pushed at `kv_cache_core.rs:149`) but is **never consulted** by
the write path (`kv_cache_core.rs:121` → `write_to_page(seq_pos, ...)`), the read
path (`kv_cache_ops.rs:53-54` `page_table.get(pos)`), or the attention path
(`kv_cache_ops.rs:96-101` `page_table.get(pos)`).

Failing oracle: `core-runtime/tests/kv_cache_test.rs::test_multi_sequence_independence`
— seq1 reads back seq2's data (`Seq1 key mismatch: got 20`, expected ~10).

Secondary defects in scope:
- **Read leak**: `read_from_page_table` (`kv_cache_ops.rs:47-62`) and
  `attention_from_pages` (`kv_cache_ops.rs:90-104`) both index by global `pos`.
- **Remanence**: `Page::reset` (`paged.rs:64-66`) zeroes only `used_slots`, not
  the key/value buffers. A reused page retains prior tenant bytes.
- **Eviction hazard**: `evict_pages_before` (`kv_cache_ops.rs:124-139`) drains a
  sequence's `page_ids` and frees them; under the current sharing a freed page
  may still be referenced by another sequence (use-after-free of data).
- **Lock inversion** (pre-existing, fix while here): `allocate_page_for_seq`
  (`kv_cache_core.rs:131-152`) holds the `page_table` lock (from `:136`) then
  acquires `sequences` (`:147`), whereas `evict_pages_before` acquires
  `sequences` (`:125`) then `page_table` (`:136`). Opposite orders → deadlock risk.

Note the `advanced` feature path (`Q8KvStore` quant store) has a **private
per-sequence store** and masks the bug — which is why it only reproduces under
default features.

---

## 2. Chosen design — PageTable becomes a pure page pool (Option A)

**Principle**: physical pages are owned exclusively by exactly one sequence.
The global position→page map is the bug; remove it. Sequences resolve their own
pages through `entry.page_ids`, and the `PageTable` only allocates/frees/returns
pages *by `PageId`*.

This was the audit-endorsed direction (Entry #82 decision line: "exclusive
per-sequence page ownership … route lookups via page_ids").

### 2.1 PageTable API changes (`core-runtime/src/memory/paged.rs`)

Add pure-pool accessors and a pool allocator; deprecate the position-keyed ones.

```rust
impl PageTable {
    /// Allocate a fresh (or recycled) page for exclusive caller ownership.
    /// Replaces `allocate(seq_pos)` — no global position dedup.
    pub fn allocate_page(&mut self) -> Option<PageId> {
        self.get_or_create_page()          // existing helper, paged.rs:150-160
    }

    /// Borrow a page by its id (O(1); id.0 is the pages-vec index).
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.get(id.0)
    }

    /// Mutably borrow a page by its id.
    pub fn page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.pages.get_mut(id.0)
    }
}
```

- **Remove** the `entries: Vec<Option<PageId>>` field (`paged.rs:72`) and every
  use: `allocate` (`:94-105`), `get`/`get_mut` (`:124-137`), `ensure_entries`
  (`:144-148`), and the `entries` sweep in `free` (`:116-120`). `free` keeps only
  the page-reset + free-list return loop (`:108-115`).
- If a smaller blast radius is preferred, KEEP `entries` but stop using it for
  correctness — mark `allocate`/`get`/`get_mut` `#[deprecated]` and route all
  callers to the new by-id API. Primary recommendation: remove, because a dead
  global map invites regression.

### 2.2 Page remanence hygiene (`paged.rs`)

```rust
pub fn reset(&mut self) {
    self.used_slots = 0;
    self.keys.iter_mut().for_each(|x| *x = 0.0);   // zero prior-tenant bytes
    self.values.iter_mut().for_each(|x| *x = 0.0);
}
```

### 2.3 Write path (`core-runtime/src/memory/kv_cache_core.rs`)

`append_kv` (`:97-129`) and its helpers must resolve the page through the
sequence's own `page_ids`, under the resolve-drop-acquire lock discipline.

- `allocate_page_for_seq` (`:131-152`): call `page_table.allocate_page()` (pool),
  push the returned id into `entry.page_ids` (already done at `:149`). Acquire the
  `page_table` lock, allocate, DROP it, then acquire `sequences` to push the id —
  never hold both in the page_table→sequences order.
- `write_to_page` (`:154-159`): change signature from `(seq_pos, slot, ...)` to
  `(page_id: PageId, slot: usize, ...)` and use `page_table.page_mut(page_id)`.
- In `append_kv` Phase 3 (`:120-121`): resolve
  `let page_idx = seq_pos / PAGE_TOKENS;` then read `entry.page_ids[page_idx]`
  under the sequences lock, drop it, then call `write_to_page(page_id, seq_pos % PAGE_TOKENS, keys, values)`.

### 2.4 Read + attention paths (`core-runtime/src/memory/kv_cache_ops.rs`)

- `read_kv` (`:11-45`): while holding the sequences lock, compute
  `page_id = entry.page_ids[pos / PAGE_TOKENS]` (bounds-check; return
  `KvCacheError::PageNotFound` if the index is out of range). Drop the lock, then
  call a revised `read_from_page_table(page_id, pos % PAGE_TOKENS, keys_out, values_out)`.
- `read_from_page_table` (`:47-62`): take `(page_id, slot)`, use `page_table.page(page_id)`.
- `attention_scores` (`:65-88`) / `attention_from_pages` (`:90-104`): resolve the
  sequence's `page_ids` (clone the `Vec<PageId>` under the sequences lock), drop
  the lock, then iterate `pos in 0..seq_len`, mapping
  `page_id = page_ids[pos / PAGE_TOKENS]`, `slot = pos % PAGE_TOKENS`,
  `page_table.page(page_id)`.

### 2.5 Eviction (`kv_cache_ops.rs:124-139`)

`evict_pages_before` already drains `entry.page_ids` and frees exactly those ids.
Under exclusive ownership this is now correct (no other sequence references them).
Keep the sequences→page_table lock order here; §2.3 makes the write path match it.

---

## 3. Test oracles (write/confirm BEFORE implementing — TDD)

All in `core-runtime/tests/kv_cache_test.rs` unless noted. Run with default
features (NO `--features advanced`, which masks the bug).

1. **`test_multi_sequence_independence`** (EXISTING, currently FAILING): must pass
   unchanged — seq1 reads 10.0/100.0, seq2 reads 20.0/200.0.
2. **NEW `test_two_sequences_same_position_distinct_pages`**: allocate seq1, seq2;
   append one token each at pos 0; assert `sequence_page_count(seq1)==1`,
   `sequence_page_count(seq2)==1`, and the two `page_ids[0]` differ (add a test
   accessor or assert via distinct read-back values).
3. **NEW `test_evicted_page_is_zeroed` (paged.rs unit test)**: allocate a page,
   write data, `free` it, re-allocate; assert `read_keys(0)` returns all zeros
   (remanence hygiene, §2.2).
4. **Update PageTable unit tests** in `paged.rs` (`test_page_table_free`,
   `test_page_table_reuse`, `test_page_table_free_multiple`, `test_page_table_basic_allocation`):
   they call the removed `allocate(seq_pos)`/`get(seq_pos)` API. Rewrite against
   `allocate_page()` + `page(id)`. This is expected churn, not scope creep —
   note it in the plan's Affected Files.
5. Full-suite regression: `kv_cache_test.rs` must reach 14/14; no other suite
   may regress (compare against the cycle-2 baseline in the runbook doc).

Each test must assert a **value**, not mere presence (SG-035): "if isolation
were silently broken but the test existed, would it fail?" — yes for all above.

---

## 4. FEATURE_INDEX obligation

`docs/FEATURE_INDEX.md` row **F-21** is currently `unverified` (set during cycle
2 precisely because of this bug). On completion, flip F-21 to `verified` in the
SAME commit, test path `core-runtime/tests/kv_cache_test.rs`.

---

## 5. Definition of Done

- **D1**: no cross-sequence KV visibility under default features; pages are
  exclusively owned.
- **D2**: `paged.rs` is a pure pool (`allocate_page`/`page`/`page_mut`, global
  `entries` removed); `kv_cache_core.rs` + `kv_cache_ops.rs` resolve via
  `page_ids`; `Page::reset` zeroes buffers; lock order is uniformly
  sequences→page_table.
- **D3**: BACKLOG B-20 → done; FEATURE_INDEX F-21 → verified (same commit);
  META_LEDGER seal entry; GitHub #58 gets an evidence comment (do NOT close it —
  operator closes).
- **D4**: `cargo test --workspace --no-fail-fast` — `kv_cache_test` 14/14 incl.
  the new isolation + remanence tests; no regression elsewhere.

## 6. CI Commands (run serialized — see runbook §Gotchas)

- `cargo test --workspace --no-fail-fast`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`

## 7. Boundaries

- Do NOT touch the `advanced`/`Q8KvStore` quant path — it is already isolated.
- Do NOT change public IPC or engine signatures.
- Stay within `core-runtime/src/memory/` + `core-runtime/tests/kv_cache_test.rs`.
- This is the ONLY change in its cycle — do not fold in ADR-007 or clippy work.
