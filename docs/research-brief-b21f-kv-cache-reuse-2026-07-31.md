# Research Brief — B-21f: KV-Cache Reuse Across Speculative Steps (the real speedup)

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-21f — make the wired GGUF speculative path *actually faster* by reusing the llama.cpp
KV cache across draft/verify steps instead of rebuilding a fresh `LlamaContext` (and re-decoding the
whole context) every step. The B-21c wiring is correct but net-slower → `auto_disable` fires; B-21f is
the backend change that removes that penalty.
**Scope**: `engine/gguf/backend.rs` (`LlamaBackendInner`, `create_context`, `verify_tokens`,
`generate_from_tokens`), `engine/gguf/generator.rs` (the stateless speculative shims),
`engine/gguf/adaptive_speculative.rs` (the B-21c adapter), `engine/adaptive_speculative/executor.rs`
(the loop that owns `context`), the `llama-cpp-2 = 0.1.133` KV API. Read-only.

---

## Executive Summary

KV-cache reuse is **feasible and API-supported** in `llama-cpp-2 0.1.133`: `LlamaContext` exposes
`clear_kv_cache_seq(seq, p0, p1)` (the rejected-suffix rollback via `llama_memory_seq_rm`),
`clear_kv_cache`, `kv_cache_seq_pos_max`, and incremental `decode` (KV accumulates by batch position).
A **local GGUF model is present** (`models/qwen2.5-0.5b-instruct-q4_k_m.gguf`), so B-21f is
**verifiable locally** (token-equivalence + reduced decode count) even though CI has no model. **The
load-bearing obstacle is architectural, not API**: `LlamaBackendInner` owns `model: LlamaModel` by
value and `create_context(&self) -> LlamaContext<'_>` returns a context that *borrows* it, so a
persistent context that survives across decode steps is a **self-referential borrow** — the exact
reason today's `verify_tokens`/`generate_from_tokens` each build a throw-away context in a scoped
`&self` method. Reusing KV therefore requires holding the model + its context together (a
self-referential struct via `self_cell`/`ouroboros`, or restructuring the loop to own both in one
scope). Two design axes need an operator decision (below): **how** the persistent context is held vs
the stateless executor traits, and **scope** — whether B-21f also provisions a distinct draft model
(without one, same-model self-speculation is always-accept → B-21f removes the slowdown but yields no
headline >1× speedup).

## Findings (verified, file:line-grounded)

### F1 — the KV API is fully present (`llama-cpp-2 0.1.133`)
`context/kv_cache.rs`: `clear_kv_cache_seq(src: Option<u32>, p0: Option<u32>, p1: Option<u32>) ->
Result<bool, KvCacheConversionError>` (wraps `llama_memory_seq_rm`; `clear_kv_cache_seq(Some(0),
Some(p), None)` drops positions `[p, ∞)` for seq 0 — the reject-suffix rollback), `clear_kv_cache`,
`llama_kv_cache_seq_keep`, `kv_cache_seq_add`, `kv_cache_seq_pos_max`. `context.rs`: `decode(&mut
batch) -> Result<(), DecodeError>` accumulates KV by position across successive calls (already relied
on by `sample_loop`/`generate_from_tokens`, which decode the prompt once then decode one token per
step within *one* context). So within a single persistent context, incremental decode + suffix
rollback is exactly the primitive set speculative KV reuse needs.

### F2 — today's speculative calls are stateless and rebuild everything
`backend.rs:207 verify_tokens(context, draft)`: `create_context()` (fresh) → decode `context+draft`
(ALL positions) → greedily sample at each draft position → `accept_all`/`diverge_at`. `backend.rs:175
generate_from_tokens(context, count)`: fresh context → decode `context` → autoregress `count`. **Each
call re-decodes the entire `context` from position 0** — O(|context|) wasted work per step, ×2 (draft
+ verify). That is precisely the cost that makes the wired path net-slower than single-model and trips
`auto_disable` (threshold 1.05).

### F3 — the executor is stateless-by-design (context-slice passing)
`executor.rs:54 run` keeps `context: Vec<u32>` and passes the FULL growing slice to every
`drafter.draft(context, k)` / `verifier.verify(context, block, plan)` / `generate_one(context)` call
(`BlockDraftModel`/`TargetVerifier` take `context: &[u32]`, `&self`, async). Critically the executor
**only ever pushes committed tokens** (`step` returns `into_tokens` = accepted prefix + optional
correction; the rejected suffix is never pushed). So the sequence of `context` values seen by the
backend is monotonically growing and always the committed prefix — a persistent KV keyed to that
prefix only needs to (a) decode the delta `context[kv_pos..]` each call and (b) roll back the
speculative draft positions it added during a verify so `kv_pos` stays at the committed length.

### F4 — the architectural obstacle: self-referential context lifetime
`backend.rs:22 LlamaBackendInner { backend, model, n_ctx, n_threads }` owns the model by value;
`create_context(&self)` → `.new_context(&self.backend, p)` returns `LlamaContext<'_>` bound to `&self`.
A context that persists across steps must be stored *with* the model it borrows → a self-referential
struct, which Rust forbids directly. Options: (i) a `self_cell`/`ouroboros` `GgufSpeculativeSession`
holding `Arc<LlamaBackendInner>` + `LlamaContext<'that>`; (ii) run the whole speculative loop inside a
single `&self` backend method that owns the context in-scope (inverts control flow — backend drives the
loop, losing the executor's telemetry/scheduler/policy integration); (iii) unsafe lifetime extension
(rejected — brittle). The generator's `Arc<LlamaBackendInner>` (`inner`) + the `&self` async trait
shape mean the session needs interior mutability (`Mutex<GgufSpeculativeSession>`) to satisfy
`BlockDraftModel`/`TargetVerifier`'s `&self`.

### F5 — the same-model caveat (no distinct draft ⇒ no headline speedup)
The repo ships one model (qwen2.5-0.5b). B-21c's `register_draft_pair(target, draft)` allows distinct
ids, but with draft == target the verifier's greedy prediction always equals the draft ⇒ `accept_all`
every step. KV reuse then makes the path *no longer slower* (removes the per-step rebuild) but a real
>1× speedup needs either a genuinely cheaper draft (a smaller draft model, or a model-free draft such
as prompt-lookup/n-gram) so draft cost ≪ target verify. This is a scope decision, not a blocker: B-21f
is the necessary enabler regardless (without it even a good draft can't win).

### F6 — verification is local-only (model not in CI)
`tests/e2e_model_test.rs:18` loads `../models/qwen2.5-0.5b-instruct-q4_k_m.gguf` via `load_test_model()
-> Option<..>`; every model test early-returns when absent. The model is NOT in CI, so B-21f's
runtime correctness (KV-reuse tokens == fresh-context tokens) + the decode-count reduction are proven
**locally with the qwen model**; CI builds the `gguf` leg but exercises none of it. Same CI-gated-by-
construction posture as B-21b/B-21c (advanced not in the CI matrix; model absent).

## Recommendations

1. **Design (recommended: F4-(i) stateful session behind the traits)** — introduce a
   `GgufSpeculativeSession` (self-referential `Arc<LlamaBackendInner>` + persistent `LlamaContext` +
   `committed_pos`) held as `Mutex<..>` inside the GGUF adapter. `draft`/`verify` decode only
   `context[committed_pos..]`, verify adds draft positions then `clear_kv_cache_seq(Some(0),
   Some(committed_pos), None)` rolls them back so the next call resumes from the committed prefix. The
   executor + traits are unchanged (all state is inside the adapter). Unit-test the position/rollback
   bookkeeping with a mock backend; verify token-equivalence + reduced decode count locally against
   qwen. L3 (touches the live inference-path backend), `all(gguf, advanced)`-gated.
2. **Scope decision for the operator (F5)** — (A) B-21f = *remove the per-step rebuild penalty only*
   (self-speculation always-accept; headline >1× deferred to a draft-model item), verifiable now; or
   (B) B-21f also lands a model-free **prompt-lookup draft** so a genuine >1× is demonstrable with the
   single local model; or (C) provision a second (smaller) draft model. (A) is the honest,
   bounded, measurement-first cut; (B) adds a real but self-contained speedup demo; (C) adds an
   external model dependency.
3. If a new dep is needed for F4-(i), prefer `self_cell` (no proc-macro, no unsafe surface) over
   `ouroboros`.

## Updated Knowledge (Shadow Genome)

**"Make it faster" can be gated by a borrow, not an algorithm.** The reason the wired speculative path
re-does O(n) work per step is not a missing API — llama-cpp-2 has full KV control — but that the
persistent artifact (the context) *borrows* the model, so the safe/simple code throws it away each
call. Recognize self-referential-lifetime obstacles as first-class design forks (self_cell vs
loop-owns-scope vs unsafe), and separate the *enabler* (persistent KV) from the *payoff* (a draft
cheap enough to win), which may need its own scope.

---

_Research complete. API + mechanism + local-model verifiability confirmed; the obstacle is the
self-referential context lifetime, and the scope fork is whether B-21f also makes a >1× speedup
demonstrable (draft strategy/model) or only removes the slowdown. Recommend a stateful
`GgufSpeculativeSession` behind the unchanged executor/traits; operator picks scope A/B/C._
