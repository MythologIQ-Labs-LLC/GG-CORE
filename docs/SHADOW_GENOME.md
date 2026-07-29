# Shadow Genome

This document records failure modes from GATE TRIBUNAL vetoes to prevent repetition of similar errors.

---

## Failure Entry #1

**Date**: 2026-02-14T16:00:00+00:00
**Verdict ID**: Entry #65 (META_LEDGER.md)
**Failure Mode**: HALLUCINATION

### What Failed

Pre-Testing Hardening Bundle - Phase 2 (IPC Binary Encoding Integration)

### Why It Failed

The Governor proposed adding V2 encoder tests to `tests/encoding_roundtrip_test.rs` that already exist in the codebase:

| Proposed Test | Already Exists At |
|---------------|------------------|
| v2_encode_empty | line 107 |
| v2_encode_single | line 115 |
| v2_roundtrip | line 128 |
| v2_decode_truncated | line 137 |
| v2_decode_length_mismatch | line 145 |
| v2_smaller_than_v1 | line 155 (as v2_vs_v1_size_comparison) |

The plan stated "V2 binary encoder exists but may not be wired into the handler. Verify integration and add benchmark comparison." but then specified tests that were already implemented.

### Pattern to Avoid

**Before proposing new tests**:
1. Use `Grep` or `Read` to verify the test file doesn't already contain the proposed tests
2. Check existing test coverage before planning new tests
3. If tests exist, acknowledge them and scope the plan to only what's missing

### Remediation Attempted

Governor must revise plan to:
1. Remove duplicate test specifications
2. Acknowledge existing V2 encoder test coverage (12 tests at lines 104-189)
3. Limit Phase 2 to benchmark comparison only (if needed)

---

## Failure Entry #2

**Date**: 2026-07-08
**Verdict ID**: Session 2026-07-08T1651-6c68b6 (GATE, cycle 2)
**Failure Mode**: HALLUCINATION

### What Failed

`docs/plan-runtime-hardening-cycle2-2026-07-08.md` — Phase 1 (KV isolation fix, #58) and Phase 3 (security-file clippy carve-out, #56).

### Why It Failed

1. **Fix asserted a false property of the code.** The plan claimed the
   `page_table.allocate(seq_pos)` key "becomes irrelevant to correctness once
   lookups go via page id." In reality `PageTable::allocate` (paged.rs:98-99)
   dedups on a GLOBAL position-indexed map and returns the EXISTING page for a
   colliding position — so two sequences at the same seq_pos receive the SAME
   page id at allocation time. Routing lookups via `entry.page_ids` cannot fix a
   collision baked in at allocation. The oracle test would still fail.
2. **Isolation deliverable claimed completeness it did not have.** D1 promised
   "no cross-sequence KV data visibility" while leaving `attention_from_pages`
   (kv_cache_ops.rs:96-101) on the position-keyed lookup — the same leak channel.
3. **Security-file lint map was fabricated.** The plan attributed a
   field-reassign-with-Default lint to `encryption_tests.rs` (file has none; its
   real lints are constant assertions including the PBKDF2 OWASP-floor oracle at
   :371, which "remove" would delete) and a "constant-assertion removal" to
   `prompt_injection.rs` (file has none; its real lint is a u8-as-u8 cast at :189
   in LIVE `scan()` code).

### Pattern to Avoid

**Before asserting a fix is sufficient**:
1. Trace the failing test through the PROPOSED code path end to end, not just the
   diagnosed bug through the CURRENT path. A correct diagnosis does not imply a
   correct fix.
2. When claiming an allocation/lookup key is "irrelevant," read the allocator's
   dedup/reuse logic — keys that gate reuse are never irrelevant.
3. Enumerate ALL read paths over the shared state (read_kv AND attention) when
   the deliverable promises isolation.
4. Map each clippy lint to its exact file:line and code region (live vs test)
   before classifying an edit as "test-only" or "behavior-preserving" — never
   classify by filename.

### Remediation Attempted

Governor must revise the plan: exclusive page ownership at allocation (drop or
per-sequence the entries dedup), route attention_from_pages via page_ids, specify
lock discipline (resolve-drop-acquire; ticket the pre-existing
page_table→sequences inversion in allocate_page_for_seq), correct the Phase 3
lint map with rework-not-remove for security-invariant assertions, fix the mmap
error-variant assertion, and complete the validate_path caller enumeration
(python/session.rs:105). Full requirements: `.agent/staging/AUDIT_REPORT.md`
(cycle 2, V1-V3 + R1-R5).

---

## Entry #3: Clippy lint mis-attribution (recurring hallucination)

**Session**: 2026-07-08T1651-6c68b6 · **Phase**: PLAN→AUDIT (cycle 2, rev.1 + rev.2)

**Pattern**: Authored the Phase-3 clippy fix map from memory instead of from
`cargo clippy` output. rev.1 fabricated the security-file lint classes; rev.2
corrected those but swapped two others — claimed `health.rs:94,95` were "manual
checked division" (actually constant-value assertions) and `stats.rs:61,66` were
"field-reassign-after-Default" (actually the manual-checked-division sites). Both
VETO'd by the Judge (Entries #82, #83).

**Root cause**: describing a mechanical lint's fix without reading the emitted
diagnostic. The lint *name* and *location* are ground truth; guessing the
transformation from the file name is a hallucination surface.

**Countermeasure**: for any clippy-cleanup plan, derive the map from captured
`cargo clippy` output — pair each `error:` line with its `-->` location, cite the
verbatim lint name, and prescribe the machine suggestion (or `--fix`). Never
infer the lint class from the file. Security/exit-convention **constant
assertions are oracles** — rework to `const _: () = assert!(...)`, never delete.

---

## Entry #4: Stale-local-main issue-state drift (near-miss)

**Session**: 2026-07-25T1224-38ccc6 · **Phase**: RESEARCH (Step 2.5 pre-check)

**Pattern**: Local `main` (`354d41d`) was ~2 weeks behind `origin/main`
(`11bf0ac`, PR #71 merge). Ancestor checks against local `main` reported the
fixes for issues #55/#56/#57/#69 as unmerged, which would have shipped a
research brief classifying four resolved issues as live P1 work. Issue bodies
and BACKLOG rows (B-17/B-18/B-19/B-22) also still described the pre-merge
state — pointer layers go stale silently after a merge.

**Root cause**: treating the local clone and issue text as ground truth for
merge state. Merge state lives on the remote; issue bodies are written once
and rot.

**Countermeasure**: during any issue-state pre-check, run `git fetch` first
and ancestor-check fix commits against `origin/main` (never local `main`);
cross-check `gh pr list --state merged` and the CI run on the merge commit
before classifying an issue as open work.

---

## Entry #5: "Exists and tested" mistaken for "wired into production" (latent blueprint drift)

**Session**: 2026-07-25T1233 (research) · **Phase**: RESEARCH

**Pattern**: Two subsystem families were represented — in ARCHITECTURE_PLAN's
data flow, FEATURE_INDEX narrative, and the same-day ledger-#96 research
brief — as active production machinery, but production-path call-site
verification shows zero callers: (1) the in-house perf kernels
(`engine/flash_attn.rs`, `engine/simd_matmul.rs`, `memory/paged.rs`,
`memory/kv_quant.rs`) are `advanced`-gated and never invoked by the GGUF
decode path, which runs entirely inside llama-cpp-2 (`backend.rs:296`); and
(2) the security interception chain (`PromptInjectionFilter`, `PIIDetector`,
`OutputSanitizer`) has no call sites in `engine/` — only admission control
runs (`worker.rs:117-127`). Modules compile, unit tests pass, FEATURE_INDEX
rows read "verified" — yet the production request never touches them.

**Root cause**: verification bound to "module has a passing test binding,"
not "production call path reaches the module." Documentation then compounds
the error by describing intent as behavior.

**Countermeasure**: before citing any module as active in a brief, plan, or
index row, grep for production call sites (who calls it, from which entry
point) and cite the call chain — not the module's existence or its tests.
This is the concrete execution of BACKLOG B-14 / SG-035 (deep-verify
FEATURE_INDEX `verified` rows). Security-surface instances of this pattern
are L3 findings, not documentation nits.

---

## Entry #6: Regex-only PII redaction has a permanent NER-class blind spot

**Session**: 2026-07-25T1354 (research) · **Phase**: RESEARCH

**Pattern**: GG-CORE's egress PII redaction is documented as "enhanced
security," but the detector is pure regex + Luhn + NFKC over 13
format-structured types (`pii_patterns.rs:7-33`). It is *structurally
incapable* of catching the PII class that has no fixed format — PERSON names,
prose LOCATION/GPE references, NRP — which is precisely the PII most common in
free-form LLM output. Microsoft Presidio catches these only via an NER model
(spaCy `en_core_web_lg`); no regex can. Asserting "we redact PII" without
qualifying it as regex-grade, and without a measured precision/recall number,
overstates the protection.

**Root cause**: conflating "a redaction stage exists" with "PII is reliably
removed." Format-tractable PII (SSN, card, email) ≠ all PII.

**Countermeasure**: qualify egress-redaction claims as regex-grade until a
span-level precision/recall/F1-per-type harness (Presidio-standard, over a
vendored offline corpus) puts a number on them. The NER-class gap closes only
with a model — an offline ONNX NER model on the candle-onnx path (couples to
issue #72's tokenizer work), never an in-process Python or HTTP-sidecar
Presidio (both violate the offline/no-network/no-in-process-Python charter).

---

## Entry #7: CI-invisible surfaces accumulate compile debt that plans inherit

**Session**: 2026-07-25T1420-facade · **Phase**: PLAN→AUDIT (iter 2 VETO, Entry #101)

**Pattern**: A plan to route FFI/Python through a new secure façade was VETO'd
when the audit found the FFI surface already carried latent compile defects: a
non-exhaustive `From<InferenceError>` match (`ffi/error.rs:130-135` omits
`MemoryExceeded`) and a Razor overage (`ffi/inference.rs` 272 > 250). These
compile/lint today ONLY because `.github/workflows/rust.yml` builds
default-features (`default = []`) and never compiles `ffi`/`gguf`/`python`. Any
change touching those surfaces inherits the debt, and no CI leg would catch the
breakage — the plan's DoD ("passes CI") was unverifiable because the legs it
named don't exist.

**Root cause**: treating a feature-gated surface as if it were on the verified
path. If CI doesn't build a feature, that feature's files are unmaintained w.r.t.
Razor/exhaustiveness/compilation, and a plan that modifies them is planning
against unverified ground.

**Countermeasure**: before planning changes to a feature-gated surface, confirm
CI actually builds that feature; if not, the FIRST deliverable is the CI leg
(so the work is verifiable), then remediate the pre-existing defects the leg
newly exposes, THEN make the change. Never assert a DoD against a CI leg that
does not exist. Pairs with the open-issues research P1 finding (CI lacks
gguf/onnx/python legs) and with [[stale-local-main-drift]]-class "verify the
ground before building on it" discipline.

---

## Entry #8: deprecation-shim ≠ trait-parity in a dependency major bump

**Session**: 2026-07-27T-rand09 · **Phase**: RESEARCH (rand 0.8→0.9)

**Pattern**: The `rand` 0.8→0.9 bump reads as "low severity" and mostly renames
(`thread_rng`→`rng`, `gen_range`→`random_range`) that survive as `#[deprecated]`
shims. But hidden underneath, `rand_core` 0.9 removed `impl RngCore for OsRng`,
leaving only `impl TryRngCore for OsRng` (`rand_core-0.9.3/src/os.rs:83`). Our
security module calls `OsRng.fill_bytes(..)` in 7 places (key/nonce/salt
generation) — an infallible method that no longer exists on `OsRng` in 0.9. A
naive "fix the deprecation warnings and ship" pass would fix the renames, still
fail to compile on the trait change, and — worse — a careless remedy could
downgrade a CSPRNG call site.

**Root cause**: assuming a symbol that is still *callable* (deprecated shim)
implies the *traits* other call sites depend on are also intact. Deprecation
warnings are visible; a moved trait impl is invisible until you read the trait
surface, and the compiler only reports it once the shim warnings are cleared.

**Countermeasure**: for any dependency major bump touching a security path, grep
the ENTIRE trait/method surface actually used (`fill_bytes`, `RngCore`,
`try_fill_bytes`, `OsRng`, …) against the vendored source of the *target*
version, not the changelog or docs. Preserve semantics with the crate's blessed
adapter (`TryRngCore::unwrap_err()` → infallible `RngCore`, panic-on-entropy-
failure) rather than hand-rolling error handling on a crypto path.

---

## Entry #9: an unwired surface can hide a deeper unwiring

**Session**: 2026-07-28T-b29-onnx-dispatch · **Phase**: RESEARCH (B-29)

**Pattern**: B-29 was framed as "add manifest-driven selection over the ONNX
loaders" — implying the loaders were reachable and only the *selection* was missing.
Investigation found the loaders (`load_onnx_classifier`/`load_onnx_model`) have
**zero** production callers (both prod load sites hard-code `load_gguf_model`), and
the engine registry is typed `Arc<dyn GgufModel>` (`inference.rs:19`) — an ONNX model
(`Arc<dyn OnnxModel>`, a different trait) cannot even be stored in it. So the stated
gap ("no selection") was the visible tip of a larger one: ONNX has no entry point and
no registry home. A dispatcher built to the literal ask would compile, pass its unit
tests, and still not make ONNX servable.

**Root cause**: scoping an "add X over Y" item without tracing Y all the way to a
live entry point and a concrete home type. The backlog row described the symptom
(selection absent) not the cause (surface unwired end-to-end).

**Countermeasure**: before scoping an integration item, grep for a *production*
caller of the thing you're extending (exclude its own module + tests) and confirm the
value it returns has a place to live (registry/trait). If either is missing, the real
work is wiring/abstraction, not the stated feature — split accordingly and say so.
Pairs with [[exists-tested-not-wired]] (this is its registry-level sibling).

---

### Entry #10: plan inlined tests into an already-half-full module → Razor VETO

**Context**: B-29a plan (manifest-driven ONNX dispatch), GATE VETO, Entry #124.

**Failure**: The plan added ~70 lines of dispatch code **and ~10 inline unit tests**
to `onnx/mod.rs`, which already stood at 128 lines. Projected total ≈274 — over the
Section 4 Razor 250-line file limit. The plan defaulted to "tests module in mod.rs"
without counting the resulting file size, and without noticing the repo already
solved this exact problem next door: `onnx/classifier.rs` (221 lines) externalizes its
tests to a sibling `classifier_tests.rs` via `#[cfg(test)] #[path=...] mod tests;`.

**Root cause**: file-size budget not computed when choosing test placement; existing
sibling-file convention not consulted before defaulting to inline tests.

**Countermeasure**: when a plan adds code + tests to an existing file, sum
(current lines + new code + new tests) against 250 *in the plan*, and prefer the
repo's established externalized-test pattern (`#[path] mod tests;` → `*_tests.rs`, or
a dedicated `<unit>.rs` submodule) whenever the total would approach the limit. A
new logical unit (here: dispatch) deserves its own file regardless.

---

### Entry #11: a "unification" epic can be a subset-promotion in disguise

**Context**: B-29b research (unify GGUF/ONNX so the registry holds ONNX), ledger #128.

**Pattern**: The B-29a brief (F2) flagged `GgufModel` and `OnnxModel` as "different
traits" with ONNX having "no registry home," framing B-29b as a large abstraction
refactor. Diffing the two trait bodies side-by-side showed `OnnxModel` is a strict
**subset** of `GgufModel` (missing only `infer_cancellable`/`set_device_placement` —
both defaulted — and `as_any`). The "big refactor" collapses to a mechanical
superset-promotion (~6 prod sites).

**Corollary (F3)**: sizing a dispatch feature also requires confirming the dispatch
*input* is loaded in production. Here the prod path (`ffi/models.rs`,
`python/session.rs`) never loads a `ModelManifest` at all — `load_metadata` synthesizes
name+size from the file — so architecture dispatch had no input regardless of trait
shape. A second blocking gap the "different traits" framing hid.

**Countermeasure**: before sizing a "unify X and Y" epic, diff the actual trait/interface
signatures (one may subsume the other) AND trace whether the feature's required inputs
are even present in the target execution path. Frame the epic from the two verified facts,
not the inherited narrative. Pairs with [[b29-onnx-dispatch-queued]].

---

### Entry #12: grounding grep matched dyn/impl but not `use` imports — caller miss recurred

**Context**: B-29b-1 plan GATE VETO (Entry #129); recurrence of the B-29a caller-miss
(SG-AffectedFilesContract-A).

**Failure**: The B-29b-1 plan deleted the `GgufModel`/`OnnxModel` traits but its
Affected Files missed two reference sites — `engine/mod.rs:94` (a `pub use onnx::{..,
OnnxModel}` re-export) and `tests/backend_test.rs:6-7` (a `use ...::{GgufModel}` /
`use ...::{OnnxModel}` import). Both are hard compile errors once the traits are gone.
The plan's grounding grep matched `dyn <Trait>`, `impl <Trait>`, and `trait <Trait>`
forms but NOT `use ...::<Trait>` imports or `pub use` re-exports — so an integration
test and a module re-export slipped through. This is the SAME class as B-29a (which
missed `tests/` `ModelManifest` struct-literal sites); the B-29b-1 plan even cited the
B-29a lesson yet under-scoped the grep.

**Root cause**: "enumerate every caller" was operationalized as "grep the usage forms I
thought of" (dyn/impl), not "grep every syntactic form that references the symbol"
(dyn/impl/trait/**use**/**pub use**/type-alias/bound).

**Countermeasure**: when deleting or renaming a trait/type/fn, the grounding grep MUST
cover the full reference-form set — at minimum `\b<Name>\b` across `src/` AND `tests/`
(and `benches/`/`examples/` if present) — then bucket the hits, not just the
dyn/impl ones. A bare-identifier grep (`grep -rn '\bGgufModel\b'`) over the whole crate
is the floor; narrower patterns are an optimization only after the bare grep's hits are
all accounted for. Applies to every "delete/rename symbol" plan. Strengthens
[[b29-onnx-dispatch-queued]] SG-AffectedFilesContract-A.

---

### Entry #13: a feature-gated unused import only CI could catch (Windows host)

**Context**: B-16 integration CI (PR #89) `features / ffi` leg failed at
`ffi/models.rs:11` — an unused `use crate::engine::gguf;` left behind by the B-29b-2
rewire (the `load_gguf_model` call was replaced with `load_model_dispatch`).

**Failure**: B-29b-2 sealed (#136) with local verification = `cargo clippy` under
**default features** on a **Windows** host. But `src/ffi/` is `#[cfg(feature="ffi")]`
and the ffi leg is a separate CI job (`cargo ... --features ffi -D warnings`). The default
clippy never compiled `ffi/models.rs`, so the stale import passed locally and only the
CI ffi leg (Linux) flagged it as an error under `-D warnings`.

**Root cause**: "clippy clean locally" was scoped to the features that actually compile on
this host. Feature-gated modules (`ffi`, and anything `#[cfg(unix)]`) are invisible to a
default-feature Windows build; a symbol whose last use is removed in such a module leaves a
dangling import no local gate sees.

**Countermeasure**: when an edit removes the last use of an imported symbol in a
**feature-gated or platform-gated** file (`#[cfg(feature=...)]`, `#[cfg(unix)]`), the
unused-import/dead-code check must be run under *that* feature/target — and on this Windows
host that is impossible for `unix`/native-dep features, so those edits are **CI-gated by
construction**: push and let the matrix verify before sealing (as B-16 did). After any
change to a `crate::engine::gguf`/`onnx` caller inside `ffi`/`python`, grep the touched
file for now-orphaned `use` lines before sealing. Relates to [[b29-onnx-dispatch-queued]].

---

_Shadow Genome tracks failures to prevent repetition. Each entry is a lesson._
