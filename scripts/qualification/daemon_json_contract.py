#!/usr/bin/env python3
"""Strict, model-free local daemon JSON checks for GG-CORE acceptance scripts.

Only validates CLI-provided JSON; it never establishes an IPC connection,
executes a model, downloads anything or qualifies a release.
"""

import json
import sys


def healthy(payload):
    return isinstance(payload, dict) and payload.get("health") == "healthy"


def ready_model(payload, model_id):
    models = payload.get("models") if isinstance(payload, dict) else None
    return isinstance(models, list) and any(
        isinstance(model, dict)
        and model.get("name") == model_id
        and model.get("state") == "ready"
        for model in models
    )


def absent_model(payload, model_id):
    """Refuse malformed registries; confirm the exact ID is absent in ALL states."""
    models = payload.get("models") if isinstance(payload, dict) else None
    return isinstance(models, list) and all(
        isinstance(model, dict)
        and isinstance(model.get("name"), str)
        and model["name"] != model_id
        for model in models
    )


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if len(args) not in (1, 2) or args[0] not in ("health", "ready-model", "absent-model"):
        print("usage: daemon_json_contract.py health | ready-model MODEL_ID | absent-model MODEL_ID", file=sys.stderr)
        return 2
    if (args[0] == "health" and len(args) != 1) or (
        args[0] in ("ready-model", "absent-model") and (len(args) != 2 or not args[1])
    ):
        return 2
    try:
        payload = json.load(sys.stdin)
    except (ValueError, UnicodeError):
        return 1
    if args[0] == "health":
        return 0 if healthy(payload) else 1
    if args[0] == "ready-model":
        return 0 if ready_model(payload, args[1]) else 1
    return 0 if absent_model(payload, args[1]) else 1


if __name__ == "__main__":
    sys.exit(main())
