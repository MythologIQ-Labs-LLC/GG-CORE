"""Offline model-free config CLI conformance against a real GG-CORE executable.

Validates source-independent fail-closed public process behavior only.
"""
import os
import tempfile
from pathlib import Path

from cli_contract import CliContractError, invoke


def require(result, ok: bool, marker: str, context: str) -> None:
    if ok:
        if result.returncode or result.stderr or marker not in result.stdout:
            raise CliContractError(f"{context}: expected successful public output")
    elif (result.returncode == 0 or result.stdout or
          "ERROR:" not in result.stderr or marker not in result.stderr):
        raise CliContractError(f"{context}: invalid config was not refused")


def check_config_contract(binary: Path) -> dict:
    """Exercise valid settings and invalid explicit limits in both CLI commands."""
    env = {k: v for k, v in os.environ.items()
           if not k.startswith("GG_CORE_") and k != "CORE_AUTH_TOKEN"}
    invalid = (
        ("zero_concurrency", {"GG_CORE_MAX_CONCURRENT": "0"}, "GG_CORE_MAX_CONCURRENT"),
        ("invalid_context", {"GG_CORE_N_CTX": "not-an-integer"}, "GG_CORE_N_CTX"),
        ("contradictory_memory", {
            "GG_CORE_MAX_TOTAL_MEMORY": "1048576",
            "GG_CORE_MAX_MEMORY_PER_CALL": "2097152",
        }, "GG_CORE_MAX_MEMORY_PER_CALL"),
    )
    with tempfile.TemporaryDirectory(prefix="ggcore-config-contract-") as cwd:
        require(invoke(binary, ("config", "validate"), env, cwd), True,
                "Configuration is valid.", "default config validate")
        allowed = dict(env, GG_CORE_MAX_CONCURRENT="3")
        require(invoke(binary, ("config", "validate"), allowed, cwd), True,
                "Configuration is valid.", "override config validate")
        require(invoke(binary, ("config", "show"), allowed, cwd), True,
                "GG_CORE_MAX_CONCURRENT=3", "override config show")
        for name, overrides, marker in invalid:
            rejected = dict(env, **overrides)
            for cmd in ("validate", "show"):
                require(invoke(binary, ("config", cmd), rejected, cwd), False,
                        marker, f"{name} config {cmd}")
    return {"result": "PASS", "scope": "model_free_config_cli_only",
            "checked_commands": 9, "invalid_cases": len(invalid),
            "ipc_qualified": False, "inference_qualified": False,
            "security_qualified": False}
