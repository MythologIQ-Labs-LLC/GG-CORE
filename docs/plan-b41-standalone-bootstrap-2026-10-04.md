# Plan: B-41 — Standalone bootstrap: startup preload + CLI model lifecycle (issue #106)

**Date**: 2026-10-04 · **Risk grade**: L3 (new authenticated IPC surface; model loading)
**Canonical source**: issue #106 · **Branch**: `claude/agent-memory-repo-review-lrauo3`

## Problem

The daemon starts with an empty registry; IPC has no load/unload; `models` implements
only `list`; `gg-core-cli infer` can only succeed after an in-process consumer registers
a model. Additionally (research 2026-10-04): `CliIpcClient` never sends `Handshake` and
never reads `CORE_AUTH_TOKEN`, so every authenticated command (incl. existing `infer`)
is refused by a default server (`require_auth: true`) — the CLI has no authenticated path.

## Design

### D1. Shared load/unload op (new `src/models/model_ops.rs`)

One canonical sequence, mirrored from `ffi/models.rs:18-76` (which stays untouched):
`loader.validate_path(rel)` → `load_metadata` → id = `--model-id` override or metadata
name → `load_model_dispatch(path, id)` (blocking; `spawn_blocking` from async contexts)
→ `lifecycle.load(id, metadata, model)`. Failure at any step leaves no partial state:
dispatch failures precede registration; `lifecycle.load` is already transactional
(AlreadyLoaded checked under the index write lock). Unload: `lifecycle.unload(id)`.
Runtime grows `load_model_from_path(rel_path, id_override)` / `unload_model(id)` /
nothing new for list (registry.list_models exists). `Runtime.model_loader` becomes
`Arc<ModelLoader>` (auto-deref keeps `ffi`/`python` call sites source-compatible).

### D2. IPC messages (new `src/ipc/protocol_model_types.rs` + 4 enum variants)

- `ModelLoadRequest { path, model_id: Option<String> }` / `ModelLoadResponse { success, model_id, handle_id, error }`
- `ModelUnloadRequest { model_id }` / `ModelUnloadResponse { success, model_id, error }`

Both requests **require an authenticated session** (`require_auth`), unlike
`ModelsRequest` (read-only, stays unauthenticated). Load/unload *failures* (bad path,
duplicate id, backend absent) are successful protocol responses with `success:false` +
error string — the connection stays open; only auth failures close it (existing server
behavior). Payload structs live in the new file (protocol_types.rs is over the Razor
cap); enum variants + serde tags added minimally. Codec is generic — no codec changes.

### D3. Handler dispatch (new `src/ipc/handler_models.rs` sidecar impl)

`IpcHandler` gains `model_lifecycle: Arc<ModelLifecycle>` + `model_loader:
Arc<ModelLoader>` (wired in `Runtime::init_ipc`). New match arms call
`handle_model_load` / `handle_model_unload` in the sidecar impl (handler.rs is +241
over cap; no growth there beyond the two arms + fields). Load runs dispatch via
`spawn_blocking` so the IPC task is not pinned.

### D4. CLI authenticated exchange (new `src/cli/ipc_client_auth.rs`)

`CliIpcClient::send_receive_authenticated(bytes)`: one connection — `Handshake { token:
env CORE_AUTH_TOKEN (default "") }` → `HandshakeAck` → request → response. Used by new
`models load/unload`; existing commands unchanged this cycle (CLI-infer auth fix noted
as follow-up to keep this diff reviewable).

### D5. CLI commands (`cli/models_cmd.rs` + `main.rs`)

- `models load <path> [--id ID]` → exit 0 on success (prints id + handle), 1 on
  load failure, 3 on connection failure
- `models unload <id>` → same codes
- `models list [--json]` → JSON via serde on the existing `ModelsListResponse`

### D6. Startup preload (`main.rs` + `runtime_init.rs`)

`serve [--model <path>]... [--model-id <id>]...` (manual parsing per repo convention;
k-th `--model-id` names the k-th `--model`), plus env `GG_CORE_PRELOAD_MODELS`
(comma-separated paths, flags win). After `Runtime::new`, before the server starts:
load each via D1; **any failure aborts startup** (fail-loud, exit FAILURE) — no
degraded-start mode this cycle. Paths are relative to `base_path` and constrained by
`validate_path` exactly like every other load.

### D7. Readiness

`RuntimeConfig` gains `health: HealthConfig` (default unchanged:
`require_model_loaded: false` — embedded consumers keep today's semantics).
The daemon's `load_config()` sets `require_model_loaded: true`, so standalone
readiness fails until a servable model is registered; liveness unchanged. The
verbose health report already carries `models_loaded: 0` + `Degraded` as the
actionable signal.

## Security constraints (from #106, all enforced)

No HTTP; no network model paths; caller paths bounded by `base_path` +
`validate_path` (NUL, traversal, allowlist dirs); load/unload require authenticated
sessions; manifest/backend failures fail loud; no partial registry state on failure.

## Razor compliance

All new code in new files (`model_ops.rs`, `protocol_model_types.rs`,
`handler_models.rs`, `ipc_client_auth.rs`), each ≤250. Over-cap files touched only
minimally (enum variants, match arms, struct fields). `cli_parser.rs` (245/250) help
text additions offset by compressing existing lines if needed.

## Tests

- Protocol round-trips for the 4 new variants (new `tests/model_ipc_test.rs`).
- Handler-level (no sockets, cross-platform): `Runtime::new` + `handler.process()` —
  unauthorized load → auth error; authorized load with traversal path →
  `success:false` + PathNotAllowed, connection-survivable; unload of absent id →
  `success:false`; load without a compiled backend → fail-loud error string.
- Preload arg parsing unit tests; env parsing.
- Readiness: daemon config → not ready at 0 models; ready after lifecycle load
  (MockModel pattern from `lifecycle_tests.rs`).
- Fixture-gated e2e (skips when `fixtures/models/` absent, consistent with existing
  practice): real GGUF preload → ready → infer → unload.

## Non-goals (this cycle)

FFI/Python rewiring onto D1 (behavior-preserving dedup, separate pass); CLI-infer
authentication fix (follow-up; D4 provides the mechanism); degraded-start opt-in;
`--config` file; README/USAGE_GUIDE full reconciliation (issue #107 owns it — only
the standalone quickstart section is updated here).
