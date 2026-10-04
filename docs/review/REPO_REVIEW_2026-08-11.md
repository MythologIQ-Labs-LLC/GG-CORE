# GG-CORE Repository Review — 2026-08-11

**Scope**: Full-repo review — open issues/PRs, CI gates, codebase health, dependency
currency, and ecosystem research — to identify what "complete product" requires from here.
**Baseline**: `main` @ `f4ed6ca` (v0.8.2, post-PR #109). CI green (Rust + CodeQL),
zero open PRs, working tree clean, Merkle ledger sealed through Entry #202.

---

## 1. Executive summary

The engineering core is in strong shape: the July 2026 hardening arc (B-21 epic, secure
`Runtime::infer` façade, streaming egress sanitization, perf-gated CI, feature-matrix
linting) closed most of the debt the backlog tracked, and the governance chain is intact.
What separates the repo from a *complete product* is not the inference engine — it is
the perimeter:

1. **A 1,067-line security test suite that has never run** (build-system wiring bug).
2. **Dependency drift** — the ML backbone (candle 0.8 → 0.11, cudarc 0.12 → 0.19,
   llama-cpp-2 ~21 releases behind) and the entire crypto stack are one-to-several
   breaking versions old, with no automated advisory scanning to notice.
3. **No release engineering** — no git tags for any of 0.1.0–0.8.2, no release
   workflow, no published artifacts.
4. **Documentation that contradicts the code** — already indicted by issue #107 but
   broader than it states (SYSTEM_STATE.md, ROADMAP.md claim GPU/MoE features
   "complete" that are mocks in-tree).
5. **The standalone first-run gap** (issue #106): the daemon cannot load a model
   without a host application, so the product cannot be experienced from a clean clone.

## 2. Current state — verified

| Area | State |
|---|---|
| CI | `rust.yml` (lint 3-OS, test 3-OS, features matrix `gguf/onnx/ffi/python/advanced`, bench + 2.0× perf gate) and `codeql.yml` — **all green on main** |
| Open PRs | **None** (former stragglers #47/#59/#74 resolved) |
| Open issues | 9: #106, #107 (new, well-formed), #72, #70, #48–#52 (backend-capability epic) |
| Governance | META_LEDGER sealed to Entry #202; `feature_index_verify` 63/63; BACKLOG reconciled 2026-08-01 |
| Tests | 708 unit + 727 integration tests declared; 13 benches, all CI-safe ones gated |
| Release | v0.8.2 in Cargo.toml; large `[Unreleased]` CHANGELOG section; **zero git tags ever** |

**Stale open issues (close-ready):**
- **#72** (ONNX backend stub) — B-27/B-28/B-29a/b-1/b-2 all sealed; CHANGELOG says
  "closes #72 scope-3". Nothing remains; close with a pointer to the seals.
- **#52** (benchmark harness) — B-06 marked done, folded into B-34/B-34b CI bench gate.
  Close or re-scope to the GPU-host benchmark that B-21e still needs.

## 3. Findings

### 3.1 Critical

**C1 — The `tests/security_audit/` suite is dead code.**
`core-runtime/tests/security_audit/` (auth_attacks, boundary_tests, crypto_tests,
ipc_fuzzing — 1,067 lines of penetration tests) is rooted at `mod.rs`. Cargo only
auto-discovers `tests/*.rs` and `tests/<dir>/main.rs`; there is no `[[test]]` section
and no top-level file declaring the module. **These tests have never compiled or run,
locally or in CI**, while CI reports green and SECURITY.md claims "full coverage".
Fix is one rename (`mod.rs` → `main.rs`) plus whatever rot compilation then surfaces.

**C2 — `.cargo/config.toml` is invalid and silently ignored.**
It contains a `[dependencies]` table (`bindgen = "=0.69.4"`, `clang-sys = "=1.8.1"`),
which is not valid in a Cargo *config* file. Whatever build break those pins were meant
to prevent is unenforced. Move the pins to `Cargo.toml`/lockfile or delete the file.

**C3 — No supply-chain gate.**
No `cargo audit`/`cargo deny` job, no `deny.toml`, no `.github/dependabot.yml`.
RUSTSEC handling has been manual and retroactive (pyo3, rand advisories caught late).
For a security-positioned runtime shipping AES-GCM/PBKDF2/seccomp, this is the largest
CI hole — and `cargo-deny [bans]` would also machine-enforce the forbidden-dependency
list that today is only a comment block (Cargo.toml:224–228).

### 3.2 High

**H1 — ML backbone dependency drift** (all "latest" verified against crates.io 2026-08-11):

| Dep | Pinned | Latest | Notes |
|---|---|---|---|
| candle-core / candle-onnx | 0.8 | **0.11.0** (2026-06) | 3 breaking minors; ~14 months of ONNX op/quant coverage — directly relevant to #72's successors |
| cudarc | 0.12 | **0.19.8** | 7 breaking minors; gates modern CUDA APIs |
| llama-cpp-2 | =0.1.133 | 0.1.154 | each release tracks newer upstream llama.cpp (FP4 quants, `--spec-*` speculative rework, MTP drafters live upstream) |
| tokenizers | 0.21 | 0.23.1 | 2 breaking minors |
| metal | 0.28 | 0.33 | plus: never compiled by CI (see H4) |
| sha2/aes/aes-gcm/pbkdf2 | 0.10/0.8/0.10/0.12 | 0.11/0.9/0.11/0.13 | whole RustCrypto generation moved together — migrate as one batch (touches model encryption + hashing) |
| thiserror / toml / rand / metrics | 1.0 / 0.8 / 0.9 / 0.22 | 2.0 / 1.1 / 0.10 / 0.24 | mechanical but breaking |
| pyo3 (+async-runtimes) | 0.29 | 0.29.2 | **current** — the one fast-mover already tracked; nit: `abi3-py38` floor targets EOL Python, raise to py310 |

**H2 — ROADMAP/docs claim completeness the code disclaims.**
`ROADMAP.md` marks "GPU support (CUDA/Metal) — Complete", but in-tree:
`gpu_allocator.rs` CUDA/Metal allocators are bookkeeping mocks (TODOs at :127/:192),
flash-attention kernels are fail-loud `not implemented` (flash_attn_gpu.rs:231/:330),
and all multi-GPU execution paths are simulated (`MockPartitionExecutor`). The code's
fail-loud honesty is good; the docs' claims are not. This extends issue #107's scope.
`SYSTEM_STATE.md` (2026-07-08) describes branch topology that no longer exists.

**H3 — Perf/e2e tests that assert on synthetic numbers or always skip.**
`baseline_comparison_test.rs` and `tier2_onnx_classification_test.rs` "measure"
hardcoded simulated timings. The fixture-gated e2e tests skip silently when
`fixtures/models/` is absent — which it always is in CI. Net effect: several
green checks verify nothing.

**H4 — CI matrix holes.** `cuda`, `metal`, `full`, `gpu` features are never compiled
anywhere (a known latent unused-variable warning in flash_attn_gpu.rs:323 would fail
`-D warnings` on macOS+metal today). No MSRV pin (`stable` floats), no
`--no-default-features` leg, no coverage tooling of any kind, no job timeouts, and the
perf gate silently self-disables on baseline cache eviction.

**H5 — Standalone product gap (issue #106).** Verified accurate: the daemon starts
with an empty registry, IPC has no load/unload, `models` implements only `list`.
Until this lands, GG-CORE is only consumable as an embedded library — the "clone →
serve → infer" journey does not exist.

### 3.3 Medium

- **M1 — Section 4 Razor honor-system**: 42 files exceed the repo's own 250-line
  limit (worst: `scheduler/worker_tests.rs` 654, `engine/metal.rs` 578,
  `engine/cuda.rs` 567); several production files sit at exactly 250 via sidecar
  `#[path]` splits. No tooling enforces the Razor — a small CI lint (or
  `clippy.toml` thresholds for fn length) would end the drift either way; or amend
  the rule to exempt test files, which are most offenders.
- **M2 — Orphaned modules**: `cli/deployment_debug.rs` (180 lines, references a
  Kubernetes API — also a C.O.R.E. scope question) and `models/service_routing.rs`
  (57 lines) are declared in no `mod` tree. Delete or wire.
- **M3 — Unsafe hygiene**: 22 unsafe blocks in `src/` (SIMD/sandbox/FFI, appropriately
  thin) but no `#![deny(unsafe_op_in_unsafe_fn)]` or
  `clippy::undocumented_unsafe_blocks`; 2 unexpected blocks in
  `scheduler/worker_streaming.rs` deserve a comment or removal. 233 `.unwrap()` in
  non-test src.
- **M4 — `tempfile` is a production dependency** ("# Temp files for tests") — move to
  dev-dependencies.
- **M5 — Scope tension vs CLAUDE.md**: `src/k8s/`, `src/deployment/` (canary,
  blue-green) and the shim rate-limiter live inside a runtime whose charter is "pure
  compute, no business logic". Not the named forbidden modules, but the same drift
  vector — worth an explicit ADR either blessing or evicting them.
- **M6 — Release engineering absent**: no tags, no tag-triggered workflow, no
  packaged cdylib + `gg_core.h` + Python wheel artifacts despite those being the
  product's consumable surfaces.

### 3.4 Low / hygiene

- Root cruft, all git-tracked: `protoc.zip` (2.9 MB, force-added past `.gitignore`),
  `bin/protoc.exe`, `bin/ninja.exe`, protoc's `readme.txt`, stale
  `benchmark_status.txt`, `HEARTHLINK-CORE.png` (duplicate of docs/assets logo,
  stale brand), `include/` holding protobuf well-known types (not the cbindgen
  output CLAUDE.md describes).
- The 8 root `.bat` scripts hardcode one developer's `G:\MythologIQ\CORE` paths and a
  non-existent "Visual Studio 18" — unrunnable anywhere; duplicated by
  `scripts/*.sh|ps1`. Delete or rewrite parameterized.
- `testing/scripts/*.sh` harnesses are wired to no CI workflow.
- `docs/COREFORGE Integration Notes.txt` has a space in the filename.

## 4. Ecosystem research — where the field moved (mid-2026)

Relevant to keeping the runtime competitive, in rough priority for this codebase:

1. **candle 0.9–0.11** (Jan–Jun 2026): active again; new op/quant coverage lands in
   candle-onnx 0.11. Our 0.8 pin predates the whole wave.
2. **llama.cpp 2026**: FP4 (NVFP4/MXFP4) quantization landed; speculative decoding CLI
   reworked (`--spec-*`); MTP-trained models (Qwen3.5/3.6, Gemma 4) ship built-in
   drafters — no separate draft model needed. Directly relevant to the TierSynergy
   adaptive-speculative module and B-21e.
3. **mistral.rs**: PagedAttention default-on (CUDA), FP8 KV-cache quantization
   (~halves KV memory), prefix caching integrated with paged KV — the best in-Rust
   reference for our KV pool + prompt cache design.
4. **llguidance** (guidance-ai): pure-Rust constrained decoding (~50 µs/token, JSON
   Schema + CFGs), no network deps — sandbox-compatible path to structured output,
   a feature GG-CORE currently lacks entirely.
5. **imatrix-calibrated GGUF quants** are now the distribution norm, with
   `quantize.imatrix.*` metadata keys — registry/manifest code parsing GGUF metadata
   should expect them.
6. **Burn-LM** (tracel-ai): third credible pure-Rust backend (CUDA/ROCm). Watch, not
   migrate — but it validates the backend-capability-contract epic (#48–#52) as the
   right abstraction to be building.

## 5. Recommended sequencing

**Phase 0 — Trust the green (small, do first)**
1. C1: wire `security_audit/` into the build; fix what compiles out. (L3 — audit gate.)
2. C2: fix/delete `.cargo/config.toml`.
3. C3: add `cargo-deny` (advisories + bans encoding the forbidden-dep list) +
   `dependabot.yml`; add job timeouts; make the perf gate fail loud on cache miss.
4. Close stale issues #72 and #52 with evidence pointers.
5. H3: mark simulated-timing tests as what they are (or delete); make fixture-gated
   e2e skips visible (`--skipped` reporting) so green means green.

**Phase 1 — Product completeness**
6. Issue #106 (standalone preload + CLI model lifecycle) — the highest-leverage
   single item; turns the repo into a runnable product. (L3: IPC surface change.)
7. M6: release workflow — tag v0.8.x, cut CHANGELOG, package cdylib + header +
   wheel; establishes the cadence 1.0 will need.

**Phase 2 — Documentation truth (issue #107, expanded)**
8. Rebuild ROADMAP.md from FEATURE_INDEX/BACKLOG/issues; reconcile SECURITY.md
   (name, version line, self-scored claims) and USAGE_GUIDE.md (compiling examples);
   regenerate or retire SYSTEM_STATE.md; fold in H2's GPU-claims correction.
9. M5: ADR on k8s/deployment module scope vs C.O.R.E. charter.

**Phase 3 — Currency (batched migrations, each its own PR + audit)**
10. RustCrypto generation bump (sha2/aes/aes-gcm/pbkdf2 together) — touches model
    encryption; L3.
11. candle 0.8 → 0.11 + tokenizers 0.23 (onnx feature).
12. llama-cpp-2 → latest 0.1.x (unlocks FP4 + upstream speculative improvements).
13. Mechanical set: thiserror 2, toml 1, rand 0.10, metrics 0.24, cudarc when GPU
    work resumes; raise abi3 floor to py310; MSRV pin.

**Phase 4 — Competitive features (post-currency)**
14. Structured output via llguidance (fits the offline sandbox constraint).
15. Prompt-lookup / MTP-aware drafting in adaptive speculation; B-21e GPU benchmark.
16. Backend capability contract epic (#48–#52) — ADR first, as planned.

**Explicitly deferred**: GPU kernel implementations (mocks today) until a GPU host and
the capability-contract ADR exist; multi-GPU beyond simulation; BitNet adapter (#51).

---

*Method note: dependency versions verified against crates.io on 2026-08-11; codebase
claims verified against `main` @ `f4ed6ca` (file:line references throughout). Ecosystem
items 1–6 sourced from project repos/docs; secondary sources where changelogs absent.*
