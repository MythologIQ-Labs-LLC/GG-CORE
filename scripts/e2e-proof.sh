#!/usr/bin/env bash
# GG-CORE developer smoke helper. NOT an acceptance or release certification.
# Uses only an existing authenticated LOCAL daemon and an already-staged GGUF.
# The authoritative package gate is scripts/rc-acceptance.sh (plus #219 P0).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="${MODELS_DIR:-models}"
MODEL_ID="${MODEL_ID:-qwen}"
MODEL_FILE="$MODEL_DIR/qwen2.5-0.5b-instruct-q4_k_m.gguf"
BINARY="${GG_CORE_CLI:-$ROOT/core-runtime/target/release/gg-core-cli}"

die() { printf 'SMOKE FAIL: %s\n' "$*" >&2; exit 1; }
note() { printf 'INFO: %s\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing required tool: $1"; }

note "Developer smoke only. NOT a signed/package, safety, cancellation or release PASS."
[[ -n "${CORE_AUTH_TOKEN:-}" ]] || die "CORE_AUTH_TOKEN is required for authenticated local IPC"
[[ -f "$MODEL_FILE" ]] || die "local GGUF fixture missing: $MODEL_FILE (no implicit download)"
[[ "$(head -c 4 "$MODEL_FILE")" == "GGUF" ]] || die "fixture is not GGUF (possibly a Git LFS pointer)"
need sha256sum
note "Local fixture SHA-256 (NOT proof of loaded daemon identity): $(sha256sum "$MODEL_FILE" | cut -d ' ' -f 1)"

if [[ "${SKIP_BUILD:-false}" != "true" ]]; then
  need cargo
  (
    cd "$ROOT/core-runtime"
    cargo build --locked --offline --release --features gguf --bin gg-core-cli
  ) || die "offline locked build failed"
fi
[[ -f "$BINARY" && -x "$BINARY" ]] || die "missing executable: $BINARY"
note "CLI source/feature report (unverified as matching the running daemon):"
"$BINARY" version --json || die "CLI version query failed"

status="$("$BINARY" status --json)" || die "authenticated local daemon is not responding; start it with the pinned model"
[[ -n "$status" ]] || die "empty daemon status"
models="$("$BINARY" models list --json)" || die "cannot list models through authenticated IPC"
[[ "$models" == *"$MODEL_ID"* ]] || die "required model ID '$MODEL_ID' not listed by daemon"
note "Daemon responds and lists configured model ID; this does not prove its artifact hash."

output="$("$BINARY" infer --model "$MODEL_ID" --prompt "What is 2 + 2? Answer with the digit." --max-tokens 32)" ||
  die "real-model non-streaming inference command failed"
[[ -n "${output//[[:space:]]/}" ]] || die "non-streaming returned empty output"
[[ "$output" =~ (^|[^[:digit:]])4([^[:digit:]]|$) ]] ||
  die "non-streaming answer did not contain expected digit 4"

stream="$("$BINARY" infer --model "$MODEL_ID" --prompt "Count from one to five in words." --max-tokens 64 --stream)" ||
  die "stream-mode inference command failed"
[[ -n "${stream//[[:space:]]/}" ]] || die "stream-mode command returned empty output"
note "Stream-mode CLI invocation completed; token-by-token cadence was NOT measured."

note "NOT RUN: active cancellation, client disconnect, stop tail, memory admission lifetime."
note "NOT RUN: independent model-hash custody, real GPU, installed package, cross-platform gates."
note "NOT VERIFIED: metric deltas (separate baseline and authenticated source required)."
printf 'SMOKE PASS: authenticated local CLI inference and stream-mode output only.\n'
printf 'RELEASE HOLD: use scripts/rc-acceptance.sh and P0 #219 native acceptance.\n'
