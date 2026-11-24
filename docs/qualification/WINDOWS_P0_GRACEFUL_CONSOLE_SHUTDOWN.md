# Offline Windows graceful shutdown acceptance (#219)

This is a separate, executable native qualification cell, not a release GO.
The GG-CORE daemon now listens for Windows Ctrl+Break in addition to Ctrl+C.
Win32 permits CTRL_BREAK_EVENT directed to an explicitly created console
process group; unlike Ctrl+C, it can target the daemon without broadcasting
to the parent VM operator's process group.

Official references:
https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent
https://docs.rs/tokio/latest/tokio/signal/windows/index.html

## Preconditions

Use ONLY an approved, offline GG-CORE-only Windows VM that passed the prior
native_windows_audit_gate.py exact-head source/fixture/feature matrix.
A shared console is mandatory. Never use a detached process or a new console.

Stage the exact qualified executable as gg-core-cli.exe and the approved Qwen
fixture as models/qwen/model.gguf in a separate directory. Preserve approved
Microsoft VC++ runtime dependencies in the established native VM environment.
The test verifies exact executable SHA-256, full producer Git SHA, the model's
real SHA-256 and 491400032-byte approved GGUF size. No downloads occur.

Use a fresh evidence directory outside the staged model/binary root.
Sample command, using source and executable hashes from the passing native
gate (substitute the actual full hashes):

    python -I scripts/qualification/native_shutdown_console.py --staged-root C:\out\p0-stage --expected-head <exact-source-sha> --exe-sha256 <exact-cli-sha256> --evidence-dir C:\out\p0-graceful-unique-run

## Test sequence and evidence

The probe uses an ephemeral authenticated local named pipe. It starts a real
long streaming model call and OBSERVES emitted bytes, then queues another
request under GG_CORE_MAX_CONCURRENT=1. Before sending the signal, BOTH calls
must still be pending. It delivers GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT,
daemon_pid) to the daemon's new process group, never a broadcast or force kill.

The daemon must exit within 240 seconds with code zero, both clients must
finish, the pending request must not return success, and the daemon's own
stderr must prove BOTH "Shutdown signal received, draining..." and
"Shutdown complete". A default Ctrl+Break kill, premature exit, silent
missed signal, hanging client or missing queue refusal is a FAIL.

Receipt.json reports source/binary/model identity and result, but no secret,
prompt or model text. The output-byte count/digest is retained instead of
generated text. Daemon log files stay in authorized offline evidence storage.

**Exact limitations:** Model-free CI tests validate argument/refusal semantics
on Linux, Windows and macOS; they DO NOT deliver a native signal. Even an
actual PASS here is not WER/no-crash proof, arbitrary-model readiness, native
worker/admission/memory-release certification, model-switch A→B→A acceptance
or release approval. Failure cleanup may terminate children but is never a
graceful PASS. Keep P0 #219, #184 and #137 unclosed until their respective
native installed acceptance matrices finish.
