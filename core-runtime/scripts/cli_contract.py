"""Offline black-box conformance checks for the public GG-CORE CLI (#114).

Checks only model-free CLI behavior. It does not qualify IPC, inference,
security containment, real models, or a release artifact.
"""
import os
from pathlib import Path
import subprocess
import tempfile


class CliContractError(ValueError):
    """A public CLI command failed its observable compatibility contract."""


def invoke(binary: Path, args: tuple[str, ...], env: dict, directory: str):
    try:
        return subprocess.run([str(binary.absolute()), *args], cwd=directory,
                              env=env, capture_output=True, text=True,
                              check=False, timeout=15)
    except (OSError, UnicodeError, subprocess.TimeoutExpired) as error:
        raise CliContractError(f"CLI invocation failed: {args[0]}") from error


def check_public_cli(binary: Path, expected_version: str) -> dict:
    """Exercise actual version, help and unknown-command process behavior."""
    if not isinstance(expected_version, str) or not expected_version:
        raise CliContractError("expected version must be nonempty")
    env = dict(os.environ)
    env.pop("CORE_AUTH_TOKEN", None)
    env.pop("GG_CORE_ALLOW_INSECURE_NO_AUTH", None)
    with tempfile.TemporaryDirectory(prefix="ggcore-cli-contract-") as cwd:
        for args in (("version",), ("--version",), ("-V",)):
            result = invoke(binary, args, env, cwd)
            if result.returncode or result.stdout != f"GG-CORE {expected_version}\n":
                raise CliContractError(f"version alias contract failed: {args[0]}")
        for args in (("help",), ("--help",), ("-h",)):
            result = invoke(binary, args, env, cwd)
            if (result.returncode or result.stdout or
                    "USAGE:" not in result.stderr or
                    "COMMANDS:" not in result.stderr):
                raise CliContractError(f"help alias contract failed: {args[0]}")
        args = ("__ggcore_invalid_command__",)
        result = invoke(binary, args, env, cwd)
        if (result.returncode == 0 or result.stdout or
                "Unknown command:" not in result.stderr):
            raise CliContractError("unknown command did not refuse")
    return {"result": "PASS", "scope": "model_free_cli_only",
            "checked_commands": 7, "ipc_qualified": False,
            "inference_qualified": False, "security_qualified": False}
