"""Hash, custody, offline guest and observed IPC queue gate for #219."""
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess

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



def observed_queued_depth(raw_json):
    """The daemon Full health report has live main-queue depth; not a gauge."""
    try:
        payload = json.loads(raw_json)
    except (ValueError, TypeError):
        return None
    if not isinstance(payload, dict):
        return None
    if payload.get("check_type") != "Full":
        return None
    report = payload.get("report")
    if not isinstance(report, dict):
        return None
    depth = report.get("queue_depth")
    if type(depth) is not int or depth < 1:
        return None
    return depth
