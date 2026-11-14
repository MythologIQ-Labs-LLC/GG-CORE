# Combined-tree qualification: #195 + #193 + #183 (local integration, not merged)

**Source under test:**
- **Branch:** `integrate/v1-stack-195-193-183-20261113-local`, which is local only and not pushed.
- **Head:** commit `69fa860a365b2e50d58ca6f34a321693c0694e99`, tree `7b895ad31b4df7d8a0795a2811bc9526dae0eed2`.

**Composition** (`--no-ff` merges in this order; no textual conflicts):

| Order | Commit | Contents |
|---|---|---|
| Base | `b06aaa04` | `main` |
| 1 | `4c2fb4c` | merge of #195 `5812dac277a0d8e6618c545dd17581f8e6f2bbf9` (deterministic prefill, Revision 4) |
| 2 | `e2dbcf9` | merge of #193 `1f41798142e08635c035ffed6a8bc013ed7ac4d5` (strict explicit memory caps) |
| 3 | `69fa860` | merge of #183 `31373be07c155ce651a69c8fc8517322fd3544cd` (model release tracking, remediation folded) |

**Shared files:**
- `docs/RELEASE_SUPPORT_AND_MAINTENANCE.md` (all three; separate sections).
- `core-runtime/src/main.rs` (#193 and #183). The merged dispatch keeps #193's strict startup config and #183's `models unload --wait-ms` routing.

**Host and scope:**
- **Host:** offline Windows Server 2025 build VM (12 vCPU), rustc 1.99.1. No GitHub Actions minutes were used.
- **Not in scope:** clean installed product, Linux, macOS or GPU evidence, and release approval.
- **Daemon version string:** still `GG-CORE 0.10.0-rc.1`, because the crate version has not been bumped.

## 1. Source gate (batch `stk1`, exact tree above): PASS
- **Checks:** `cargo fmt --check` passes. `clippy --all-targets -D warnings` passes for default, advanced, ffi, onnx and gguf.
- **Tests:** all runs use `GG_CORE_REQUIRE_REAL_MODEL=1`. 0 failures.

| Suite | Passed | Ignored |
|---|---|---|
| `test` | 1489 | 0 |
| `test --features advanced` | 1624 | 3 |
| `test --features onnx,gguf,ffi` | 1590 | 1 |
| Release-wait tests (`--lib release`) | 39 | 0 |
| N7 measurement (`GG_N7_MEASURE=1`, cross-model on) | 3 | 0 |

  The one ignored test in the full suite and the three in advanced were also ignored on the individual branches.
- **Release build:** `build --release --features gguf,ffi` passes. `gg-core-cli.exe` sha256 `da912fc24f36977df33210fb0b7f5b15a4315afc5b2f8e825c119b630a566fc2`, an unsigned development build.
- **First attempt, for the record:** the first `stk1` attempt was cut off when the VM shut down. The guest's evaluation-licence grace had expired. Its results were discarded, and the run above is a complete re-run of the same tree after the owner-approved rearm.

## 2. Determinism and N7 (in `stk1`): PASS
- **Determinism matrix:** 12/12 cells record exactly the planned split, including Revision 4's 568@1 → `…,19,19,18`, 100@1 → `64,18,18` and 568@2 → `…,28,28`. Output is identical with no stop, with a stop and under load.
- **Reference prefill** (Qwen, 2516 tokens): the worst chunk was 835–998 ms, with identical output across runs.
- **Cross-model** (Phi-3.5, SmolLM2; 1 and 4 threads): every remainder piece is under `(p+6)/24 × T64`, the split and output are identical under load, and the worst chunk is within 1.2 × max(1 s, worst 64-chunk).

## 3. Packaged daemon checks (release binary above, real Qwen2.5-0.5B `74a4da8c…`): PASS

### Memory cap (#193 harness, raw `V1_STACK_MEMORY_stk-g182.jsonl`)
- **Invalid settings:** all 7 are refused by `serve` (exit 1, within milliseconds), and `config validate` reports each.
- **256 MiB cap:** with only the total set, the Job Object `JobMemoryLimit` is exactly 268435456. A 491 MB model load is refused (`limit 268435456`), and the daemon stays alive.
- **2 GiB cap:** inference works.
- **Default start:** works, with the default total of 12884344832.

### Release wait and CLI exit codes (#183 harness, raw `V1_STACK_RELEASE_CLI_stk-r183.jsonl`)

| Step | Result |
|---|---|
| `--wait-ms 2000` while a stream holds the model | **exit 4**, 1 instance pending |
| Listing during the release | `state:"unloading"` |
| Held stream after reading resumed | 860 frames, clean final frame |
| Wrong token | exit 1 |
| Load / plain unload / load | 0 / 0 / 0 |
| `--wait-ms 30000` once released | **exit 0** |
| Daemon at end | alive |

### Packaged stop tail and throughput (#195 harness, raw `V1_STACK_STOP_TAIL_stk-g184.jsonl`)
C1 and C2 were re-run in the same batch.

| Daemon | Worst tail, cancel / deadline | Throughput median, auto threads (8k / 16k) |
|---|---|---|
| C1 `3fa16661…` | 3.65 s / 3.20 s | 6.87 / 6.84 s |
| C2 `d64b5662…` | 1.13 s / 1.11 s | 7.14 / 7.07 s |
| **Stack `da912fc2…`** | **0.70 s / 0.66 s** | **7.13 / 7.15 s** |

- **Gates:**
  - Worst tail ≤1.2 s: **PASS**.
  - Throughput at auto threads: **PASS**, at most +1.1% against C2 and +4.5% against C1, inside the 10% allowance.
- **Correctness:** 0 problems; every stop quiesced with `cancelled` or `deadline_exceeded`, and every recovery succeeded.
- **Output:** the hash `57822580A271832B` is identical across all three daemons.

## 4. Not tested
- Linux and macOS.
- GPU.
- Contexts above 16k.
- The clean installed product, including Windows Sandbox.
- Signed artifacts.
- An independent review of the merged tree's merge commits.
- ONNX in a packaged build: the release features are `gguf,ffi`.
