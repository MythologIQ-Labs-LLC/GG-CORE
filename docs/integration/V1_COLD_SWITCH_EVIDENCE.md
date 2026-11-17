# Cold-switch coordinator: evidence (#137 / #153)

**Branch:** `feat/v1-cold-switch-policy-20261113-no-ci`, based on `main` `1032d1ba`, whose tree is the qualified #195+#193+#183 tree `7b895ad3`.
- **Authored tests:** `3c582e65`.
- **Coordinator:** `0cf0c15` → `8ec8089` → `0c83567` (review fixes) → `ac27fab` (Clippy fixes). **Qualified commit: `ac27fab`.**

**Host and scope:**
- **Host:** offline Windows Server 2025 build VM (12 vCPU), rustc 1.99.1. No GitHub Actions minutes were used.
- **Scope:** source tests plus a real-model **in-process** switch. This is **not** authenticated IPC switching, which is not exposed pending #153. It is also not Linux, macOS, GPU, packaged-daemon or installed-product evidence.

## 1. Authored Branch A tests as received (`3c582e65`, batch `v1a`): PASS
- **Checks:** fmt passes; Clippy `-D warnings` passes for default, advanced, ffi, onnx and gguf.
- **Tests:** default 1493 passed; advanced 1628 passed, 3 ignored; full (`onnx,gguf,ffi`) 1594 passed, 1 ignored. 0 failed.
- **The four authored tests**, all run and passing:
  - `cold_a_to_b_to_a_waits_for_held_weights_and_preserves_identity`;
  - `speculative_warm_replacement_cannot_evict_current_model_to_fit`;
  - `release_timeout_cannot_be_interpreted_as_permission_to_reload`;
  - `missing_target_preflight_does_not_disrupt_current_model`.

## 2. Coordinator (`ac27fab`, batch `v1e`): PASS
- **Checks:** fmt and Clippy `-D warnings` pass for the 5 feature sets.
- **Tests:** default 1510 passed; advanced 1645 passed, 3 ignored; full 1612 passed, 1 ignored. 0 failed.
- **17 model-free coordinator tests**, all run and passing:
  - **Success and refusals:** A→B→A; stale handle; wrong source; target already loaded; same model; wait too long; unrecorded source; over-budget estimate; preflight failure.
  - **Recovery:** release timeout gives `ReleasePending`; B prepare failure leads to rollback; register resize refusal leads to rollback; B failing readiness is retired before A returns; a release timeout during recovery is reported; a changed source is never restored.
  - **Concurrency and failure:** concurrent operators are busy or bounded; a dropped caller does not interrupt the switch; a task panic reports unknown state.
  - **Sources:** a source never attaches to an unloaded handle; a bypass unload drops the source; `unload_expected` refuses a mismatch; operator load records the source and respects the authority.
- **Earlier failed batches, for the record:**
  - `v1c` (`8ec8089`): Clippy failed with `dead_code` on the deliberately unexposed coordinator and an extern-declaration clash in the test probe. Its tests passed, and so did the real switch.
  - `v1a`: the Branch B leg did not run because of a shared-runner `PATH` defect. Its results file falsely showed exit 0. Results are now judged only from test output.

## 3. Real-model in-process cold switch (batch `v1e`; raw `V1_COLD_SWITCH_REAL_v1e.txt`)
- **Fixtures** (provenance-verified, Q4_K_M), under base path `C:/ggv1mc`:
  - Qwen2.5-0.5B `74a4da8c…`;
  - SmolLM2-1.7B `77665ea4…`;
  - Phi-3.5-mini `e4165e3a…`.
- **Path:** the canonical operator path (`load_model_operated`: authority, hash before load, identity re-check, budget, register, recorded source), then `spawn_cold_switch` with a 120 s release bound.
- **In-flight request:** a 96-token greedy request on A was running when admissions closed.

| Pair | A→B release / load | Private bytes A → B → A | Budget A → B → A → end | In-flight on A | A output after return |
|---|---|---|---|---|---|
| Qwen→SmolLM2→Qwen | 1004 ms / 25.8 s | 79 → 753 → 81 MB | 485 MB → 1054 MB → 485 MB → 0 | completed once | identical (`368b1423…`) |
| Qwen→Phi-3.5→Qwen | 1010 ms / 57.5 s | 81 → 1261 → 81 MB | 485 MB → 2392 MB → 485 MB → 0 | completed once | identical (`368b1423…`) |

- **Behaviour:**
  - A is refused (closed to admissions) right after each switch.
  - B answers.
  - The return switch B→A released immediately (0 ms), because nothing held B.
  - The final unload of A released fully, `(true, 0)`, with the budget at 0.
- **Load time:** it is dominated by the hash-before-load read plus the native load. Phi-3.5 is 2.4 GB.
- **Open finding: handle count.**
  - Pair 1: baseline 79 → 92 → 105 → 105 → 104 at the end.
  - Pair 2: 104 → 105 → 117 → 117 → 116.
  - Loading a model family for the first time adds about 12 handles that are not returned at unload. Two runs, `v1c` and `v1e`, show identical counts.
  - This is consistent with one-time per-architecture native initialisation, but a leak is not excluded. Repeated same-pair cycles are needed to tell them apart. It is **not** claimed as clean resource reclamation.

## 4. Independent reviews
- **Plan L2 audit:** VETO, then PASS with conditions R1–R5, all met.
- **Implementation review:** APPROVE WITH CONDITIONS; findings 1–10 fixed in `0c83567` (see the plan's "Independent review" section).

## Not established
- Authenticated IPC switching (blocked on the #153 policy).
- GGUF↔ONNX switching.
- 1, 4 and 8 concurrent clients during a switch.
- Restart and readiness across a daemon restart.
- Wrong credentials and prohibited paths over IPC: covered by the existing #183 and #106 tests, not re-run as switch scenarios.
- Repeated-cycle handle stability.
- Linux and macOS.
- GPU.
