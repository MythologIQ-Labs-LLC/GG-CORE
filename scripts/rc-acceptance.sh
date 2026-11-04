#!/usr/bin/env bash
# GG-CORE release-package acceptance: install the packaged archive and run a
# real GGUF model through the shipped daemon + CLI with the platform's
# DEFAULT sandbox. Runs on Linux, macOS and Windows (git-bash).
#
# Usage: scripts/rc-acceptance.sh <gg-core-<ver>-<platform>.tar.gz> <base-dir>
#   <base-dir> must contain models/qwen2.5-0.5b-instruct-q4_k_m.gguf
#   (Qwen2.5-0.5B-Instruct, Q4_K_M, Apache-2.0; see MODEL_SHA256 in release.yml).
# Exit status is non-zero if any check fails. Prints PASS/FAIL/SKIP lines.
set -u
ARCHIVE="$1"; BASE="$2"
MODEL_REL="models/qwen2.5-0.5b-instruct-q4_k_m.gguf"
pass=0; fail=0
ok()   { echo "PASS: $*"; pass=$((pass+1)); }
bad()  { echo "FAIL: $*"; fail=$((fail+1)); }
skip() { echo "SKIP: $*"; }

case "$(uname -s)" in
  Linux*)  OS=linux ;;
  Darwin*) OS=macos ;;
  *)       OS=windows ;;
esac
WORK="$(mktemp -d)"
tar -C "$WORK" -xzf "$ARCHIVE"
STAGE="$(find "$WORK" -mindepth 1 -maxdepth 1 -type d | head -n1)"
if [ "$OS" = windows ]; then BIN="$STAGE/gg-core-cli.exe"; LIB="$STAGE/gg_core.dll"; else BIN="$STAGE/gg-core-cli"; LIB="$STAGE/libgg_core.$([ "$OS" = macos ] && echo dylib || echo so)"; fi

echo "== package contents =="
[ -x "$BIN" ] && ok "executable present: $(basename "$BIN")" || bad "executable missing"
[ -f "$LIB" ] && ok "native library present: $(basename "$LIB")" || bad "native library missing"
[ -f "$STAGE/gg_core.h" ] && ok "C header present" || bad "C header missing"
VER="$("$BIN" version)"; echo "  version: $VER"
[[ "$VER" == GG-CORE\ * ]] && ok "version command" || bad "version output: $VER"

# Portable timeout (macOS has no coreutils `timeout`).
# The watcher is detached from stdout/stderr: inside a $(...) capture its
# `sleep` would otherwise hold the pipe open until the deadline expires.
with_timeout() {
  local secs="$1"; shift
  "$@" & local p=$!
  ( sleep "$secs"; kill "$p" 2>/dev/null ) >/dev/null 2>&1 & local w=$!
  wait "$p" 2>/dev/null; local rc=$?
  kill "$w" 2>/dev/null; return $rc
}

export CORE_AUTH_TOKEN="rc-accept-$RANDOM$RANDOM"
unset GG_CORE_SANDBOX
if [ "$OS" = windows ]; then export GG_CORE_SOCKET_PATH="\\\\.\\pipe\\gg-core-rc-$RANDOM"; else export GG_CORE_SOCKET_PATH="$WORK/gg.sock"; fi
LOG="$WORK/daemon.log"
cd "$BASE" || exit 1
echo "== daemon (default sandbox) =="
"$BIN" serve --model "$MODEL_REL" --model-id qwen > "$LOG" 2>&1 &
SERVE_PID=$!
for i in $(seq 1 90); do grep -q "IPC server listening" "$LOG" 2>/dev/null && break; kill -0 "$SERVE_PID" 2>/dev/null || break; sleep 1; done
if ! kill -0 "$SERVE_PID" 2>/dev/null; then bad "daemon died at startup"; cat "$LOG"; exit 1; fi
ok "daemon started in ${i}s"
grep -E "Sandbox:|Auth:|Preloaded" "$LOG" | sed 's/^/  /'
case "$OS" in
  linux)   grep -q "Sandbox: SECCOMP" "$LOG" && ok "Linux default sandbox = seccomp" || bad "expected seccomp default";;
  windows) grep -q "Sandbox: ENFORCE" "$LOG" && ok "Windows default sandbox = Job Object (enforce)" || bad "expected enforce default";;
  macos)   grep -q "Sandbox: OFF" "$LOG" && ok "macOS: no daemon sandbox (documented)" || bad "expected OFF notice on macOS";;
esac
[ "$OS" = linux ] && { grep -qE '^Seccomp:\s+2' "/proc/$SERVE_PID/status" && ok "kernel reports seccomp filter mode" || bad "seccomp not active per /proc"; }

echo "== serving surface =="
"$BIN" live >/dev/null 2>&1 && ok "live" || bad "live"
"$BIN" ready >/dev/null 2>&1 && ok "ready" || bad "ready"
"$BIN" status --json 2>/dev/null | grep -q '"health"' && ok "status --json" || bad "status"
"$BIN" models list --json 2>/dev/null | grep -q qwen && ok "models list" || bad "models list"

echo "== real inference =="
OUT="$(with_timeout 180 "$BIN" infer --model qwen --prompt "What is the capital of France? Answer in one word." --max-tokens 24 2>&1)"; RC=$?
echo "  output: $(echo "$OUT" | tr '\n' ' ' | head -c 200)"
[ $RC -eq 0 ] && echo "$OUT" | grep -qi paris && ok "non-streaming inference (correct answer)" || bad "non-streaming inference rc=$RC"
SOUT="$(with_timeout 180 "$BIN" infer --model qwen --prompt "Count from one to five in words." --max-tokens 32 --stream 2>&1)"; RC=$?
[ $RC -eq 0 ] && [ -n "$SOUT" ] && ok "streaming inference" || bad "streaming inference rc=$RC"

echo "== authenticated real-model IPC controls =="
# The normal CLI smoke test establishes successful generation but does not
# exercise cancellation while native generation is active. The standard-library
# probe talks to the SAME packaged daemon through its actual socket/pipe.
if command -v python3 >/dev/null 2>&1; then PYTHON=python3
elif command -v python >/dev/null 2>&1; then PYTHON=python
else PYTHON=""
fi
if [ -z "$PYTHON" ]; then
  bad "Python 3 required for cross-platform IPC control acceptance"
elif with_timeout 210 "$PYTHON" "$BASE/scripts/rc-ipc-control-acceptance.py"; then
  ok "packaged IPC authentication, active cancel, deadline"
else
  bad "packaged IPC control acceptance"
fi
kill -0 "$SERVE_PID" 2>/dev/null && ok "daemon alive after inference" || { bad "daemon died during inference"; cat "$LOG"; exit 1; }

echo "== lifecycle + invalid loads =="
"$BIN" models unload qwen >/dev/null 2>&1 && ok "unload" || bad "unload"
"$BIN" ready >/dev/null 2>&1 && bad "ready without model" || ok "not-ready after unload"
"$BIN" models load "$MODEL_REL" --id qwen >/dev/null 2>&1 && ok "authenticated reload" || bad "reload"
"$BIN" models load "models/does-not-exist.gguf" --id ghost >/dev/null 2>&1 && bad "missing model accepted" || ok "missing model refused"
"$BIN" models load "../../etc/passwd" --id esc >/dev/null 2>&1 && bad "traversal accepted" || ok "traversal refused"
printf 'not a gguf' > models/rc-fake.gguf
"$BIN" models load "models/rc-fake.gguf" --id fake >/dev/null 2>&1 && bad "corrupt model accepted" || ok "corrupt model refused"
rm -f models/rc-fake.gguf
"$BIN" models list --json 2>/dev/null | grep -q fake && bad "partial registry state" || ok "no partial state"
OUT2="$(with_timeout 180 "$BIN" infer --model qwen --prompt "2+2=" --max-tokens 6 2>&1)" && ok "inference after reload" || bad "inference after reload: $OUT2"

echo "== auth boundary =="
CORE_AUTH_TOKEN=wrong "$BIN" infer --model qwen --prompt hi --max-tokens 2 >/dev/null 2>&1 && bad "wrong token accepted" || ok "wrong token rejected"
"$BIN" infer --model nope --prompt hi --max-tokens 2 >/dev/null 2>&1 && bad "unknown model accepted" || ok "unknown model reports error"

echo "== shutdown =="
if [ "$OS" = windows ]; then
  # $! is the MSYS pid, not the Windows PID: use MSYS kill (TerminateProcess
  # for a native child) and bound the wait so a failure cannot stall the job.
  kill -9 "$SERVE_PID" 2>/dev/null
  for i in $(seq 1 40); do kill -0 "$SERVE_PID" 2>/dev/null || break; sleep 0.5; done
  wait "$SERVE_PID" 2>/dev/null
  kill -0 "$SERVE_PID" 2>/dev/null && bad "daemon still alive after terminate" || skip "graceful console-signal shutdown is not drivable from git-bash on Windows (process terminated)"
else
  kill -INT "$SERVE_PID"; wait "$SERVE_PID"; SRC=$?
  [ $SRC -eq 0 ] && ok "graceful SIGINT shutdown, exit 0" || bad "daemon exit code $SRC (159 = SIGSYS)"
fi
( unset CORE_AUTH_TOKEN; "$BIN" serve > "$WORK/notoken.log" 2>&1 ); [ $? -ne 0 ] && grep -q "CORE_AUTH_TOKEN is empty" "$WORK/notoken.log" && ok "serve without token refused" || bad "serve without token"
echo "== RESULT: pass=$pass fail=$fail =="
[ $fail -eq 0 ]
