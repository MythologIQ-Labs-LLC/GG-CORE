#!/usr/bin/env python3
"""Fail-closed Windows native Ctrl+Break shutdown acceptance for P0 #219.

Run only inside an approved, offline, console-attached Windows VM. Requires
an already staged release CLI and the pinned real Qwen GGUF model. The
source-tree native gate and model integrity checks are prerequisites;
this script does NOT certify those gates itself.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import subprocess
import sys
import threading
import time
import uuid

APPROVED_MODEL_SHA = "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db"
APPROVED_MODEL_BYTES = 491400032
SHA40 = re.compile(r"^[a-f0-9]{40}$")
SHA64 = re.compile(r"^[a-f0-9]{64}$")
CTRL_BREAK_EVENT = 1
CREATE_NEW_PROCESS_GROUP = 0x00000200


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_inputs(root, output, expected_head, expected_exe):
    if not SHA40.fullmatch(expected_head):
        raise ValueError("Expected producer SHA must be full lowercase Git SHA")
    if not SHA64.fullmatch(expected_exe):
        raise ValueError("Expected executable SHA-256 must be 64 lowercase hex")
    if root.is_symlink() or not root.is_dir():
        raise ValueError("Staged root is missing or a symlink")
    if output == root or root in output.parents:
        raise ValueError("Evidence output must not be inside staged root")
    if output.exists() or output.is_symlink():
        raise ValueError("Evidence directory must be absent, never reused")
    exe = root / "gg-core-cli.exe"
    model = root / "models" / "qwen" / "model.gguf"
    for path in (exe, model):
        if path.is_symlink() or not path.is_file():
            raise ValueError("Required staged executable/model missing or symlinked")
    if sha256(exe) != expected_exe:
        raise ValueError("Executable digest differs from qualified exact build")
    if model.stat().st_size != APPROVED_MODEL_BYTES or sha256(model) != APPROVED_MODEL_SHA:
        raise ValueError("Unapproved model bytes/digest")
    return exe


def require_offline_windows():
    """Refuse native evidence outside a no-network-adapter Windows guest."""
    if platform.system() != "Windows":
        raise RuntimeError("Native graceful shutdown acceptance requires Windows")
    command = "@(Get-NetAdapter -ErrorAction Stop | Where-Object { $_.Status -eq 'Up' }).Count"
    result = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", command],
        capture_output=True, text=True, timeout=20, check=False)
    if result.returncode or result.stdout.strip() != "0":
        raise RuntimeError("NO-GO: Windows VM has active network adapters or cannot prove offline")


def send_group_break(pid):
    """Target ONLY the new daemon process group, never the runner's group."""
    if pid <= 0:
        raise ValueError("invalid daemon group PID")
    kernel = ctypes.windll.kernel32
    if not kernel.GetConsoleWindow():
        raise RuntimeError("NO-GO: VM executor has no shared console")
    if not kernel.GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid):
        raise OSError(ctypes.get_last_error(), "GenerateConsoleCtrlEvent failed")


def stdout_digest(process, started, summary):
    h = hashlib.sha256()
    n = 0
    with process.stdout:
        while True:
            chunk = process.stdout.read1(4096)
            if not chunk:
                break
            h.update(chunk)
            n += len(chunk)
            started.set()
    summary.update({"stdout_bytes": n, "stdout_sha256": h.hexdigest()})


def start_infer(exe, root, env, prompt, max_tokens):
    process = subprocess.Popen(
        [str(exe), "infer", "--model", "qwen", "--prompt", prompt,
         "--max-tokens", str(max_tokens), "--timeout-ms", "180000", "--stream"],
        cwd=root, env=env, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    started = threading.Event()
    summary = {}
    reader = threading.Thread(
        target=stdout_digest, args=(process, started, summary), daemon=True)
    reader.start()
    return process, started, summary, reader


def await_ready(exe, root, env, daemon, seconds=120):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if daemon.poll() is not None:
            raise RuntimeError("Daemon exited before readiness")
        try:
            probe = subprocess.run([str(exe), "ready"], cwd=root, env=env,
                                   stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, timeout=8)
        except subprocess.TimeoutExpired:
            continue
        if probe.returncode == 0:
            return
        time.sleep(0.5)
    raise RuntimeError("Packaged daemon never became model-ready")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--staged-root", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--exe-sha256", required=True)
    args = parser.parse_args(argv)
    root = args.staged_root.resolve()
    output = args.evidence_dir.absolute()
    report = {"status": "HOLD", "source_sha": args.expected_head,
              "signal": "CTRL_BREAK_EVENT", "active_started": False,
              "queued_pending_at_signal": False}
    daemon = None
    clients = []
    receipt_allowed = False
    try:
        require_offline_windows()
        exe = require_inputs(root, output, args.expected_head, args.exe_sha256)
        report["cli_sha256"] = args.exe_sha256
        report["model_sha256"] = APPROVED_MODEL_SHA
        report["model_bytes"] = APPROVED_MODEL_BYTES
        if not ctypes.windll.kernel32.GetConsoleWindow():
            raise RuntimeError("NO-GO: runner must share an interactive console")
        env = dict(os.environ)
        for name in ("GG_CORE_ALLOW_INSECURE_NO_AUTH", "GG_CORE_BUILD_COMMIT"):
            env.pop(name, None)
        env["CORE_AUTH_TOKEN"] = secrets.token_hex(32)
        # Never inherit a developer's detect-only or disabled policy.
        env["GG_CORE_SECURITY_INGRESS"] = "block"
        env["GG_CORE_SECURITY_EGRESS"] = "redact"
        env["GG_CORE_SOCKET_PATH"] = r"\\.\pipe\ggcore-shutdown-" + uuid.uuid4().hex
        env["GG_CORE_MAX_CONCURRENT"] = "1"
        env["GG_CORE_N_THREADS"] = "2"
        env["GG_CORE_N_CTX"] = "8192"
        env["GG_CORE_MAX_CONTEXT"] = "8192"
        env["GG_CORE_MAX_CONTEXT_TOKENS"] = "8192"
        env["GG_CORE_REQUIRE_REAL_MODEL"] = "1"
        version = json.loads(subprocess.check_output(
            [str(exe), "version", "--json"], cwd=root, env=env, text=True))
        if version.get("commit") != args.expected_head:
            raise RuntimeError("Packaged CLI source commit mismatch")
        report["cli_version"] = version.get("version")
        report["compiled_features"] = version.get("features")
        if not {"gguf", "onnx", "ffi"}.issubset(set(version.get("features", []))):
            raise RuntimeError("Missing expected native qualified compiled features")
        output.mkdir(parents=True)
        receipt_allowed = True
        with (output / "daemon.stdout").open("wb") as dout, \
                (output / "daemon.stderr").open("wb") as derr:
            # An attached console AND new process group are both necessary
            # for targeted CTRL_BREAK_EVENT delivery. Never DETACHED_PROCESS.
            daemon = subprocess.Popen(
                [str(exe), "serve", "--model", "models/qwen/model.gguf",
                 "--model-id", "qwen"],
                cwd=root, env=env, stdin=subprocess.DEVNULL,
                stdout=dout, stderr=derr,
                creationflags=CREATE_NEW_PROCESS_GROUP)
            report["daemon_pid"] = daemon.pid
            await_ready(exe, root, env, daemon)
            active = start_infer(
                exe, root, env,
                "Describe a river, its banks and the changing seasons in detail.",
                1200)
            clients.append(active)
            if not active[1].wait(90):
                raise RuntimeError("Active stream produced no bytes; native work not proven")
            if active[0].poll() is not None:
                raise RuntimeError("Active request already exited before shutdown test")
            report["active_started"] = True
            queued = start_infer(
                exe, root, env, "Reply with the word READY.", 10)
            clients.append(queued)
            time.sleep(0.4)
            if active[0].poll() is not None or queued[0].poll() is not None:
                raise RuntimeError("Two in-flight requests were not retained before signal")
            report["queued_pending_at_signal"] = True
            send_group_break(daemon.pid)
            report["group_break_sent"] = True
            try:
                daemon.wait(timeout=240)
            except subprocess.TimeoutExpired as error:
                raise RuntimeError("Graceful daemon shutdown exceeded 240s") from error
            report["daemon_exit"] = daemon.returncode
            for idx, (proc, event, summary, reader) in enumerate(clients):
                try:
                    proc.wait(timeout=30)
                except subprocess.TimeoutExpired as error:
                    raise RuntimeError("Client hung after daemon shutdown") from error
                reader.join(timeout=5)
                report[f"client_{idx}"] = {"exit": proc.returncode, **summary}
        # A process killed by default CTRL_BREAK handler or Kill() cannot be
        # misrepresented as gracefully drained. Verify the real serve path.
        log = (output / "daemon.stderr").read_text(
            encoding="utf-8", errors="replace")
        if ("Shutdown signal received, draining..." not in log or
                "Shutdown complete" not in log):
            raise RuntimeError("Daemon did not execute both graceful shutdown stages")
        if daemon.returncode != 0:
            raise RuntimeError("Graceful server returned nonzero exit")
        if clients[1][0].returncode == 0:
            raise RuntimeError("Queued request unexpectedly succeeded after shutdown signal")
        report["status"] = "GRACEFUL_GROUP_SIGNAL_ACTIVE_AND_QUEUED_PASS__RELEASE_HOLD"
        return 0
    except Exception as error:
        report["error"] = str(error)
        print(f"NATIVE_SHUTDOWN_HOLD: {error}", file=sys.stderr)
        return 1
    finally:
        for client in clients:
            proc = client[0]
            if proc.poll() is None:
                proc.kill()
                proc.wait()
        if daemon is not None and daemon.poll() is None:
            # Failure cleanup ONLY, never interpreted as graceful evidence.
            daemon.kill()
            daemon.wait()
            report["forced_cleanup"] = True
        if receipt_allowed:
            (output / "receipt.json").write_text(
                json.dumps(report, indent=2, sort_keys=True) + "\n",
                encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
