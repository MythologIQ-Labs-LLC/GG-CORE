#!/usr/bin/env python3
"""Strict, model-free local daemon JSON checks for GG-CORE acceptance scripts.

Only validates CLI-provided JSON; it never establishes an IPC connection,
executes a model, downloads anything or qualifies a release.
"""

import json
import sys


def unique_json_pairs(pairs):
    """Ambiguous duplicate object keys are not acceptable qualification evidence."""
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def reject_non_json_constant(value):
    raise ValueError(f"non-JSON numeric constant: {value}")


def model_entries(payload):
    """Validate the whole model list, not just a convenient matching entry."""
    models = payload.get("models") if isinstance(payload, dict) else None
    if not isinstance(models, list):
        return None
    names = set()
    for model in models:
        if not isinstance(model, dict):
            return None
        name, state = model.get("name"), model.get("state")
        if not isinstance(name, str) or not name or not isinstance(state, str) or not state:
            return None
        if name in names:
            return None
        names.add(name)
    return models


def healthy(payload):
    return isinstance(payload, dict) and payload.get("health") == "healthy"


def ready_model(payload, model_id):
    models = model_entries(payload)
    return models is not None and any(
        model["name"] == model_id and model["state"] == "ready"
        for model in models
    )


def absent_model(payload, model_id):
    """Refuse malformed registries; confirm the exact ID is absent in ALL states."""
    models = model_entries(payload)
    return models is not None and all(model["name"] != model_id for model in models)


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
        payload = json.load(
            sys.stdin,
            object_pairs_hook=unique_json_pairs,
            parse_constant=reject_non_json_constant,
        )
    except (ValueError, UnicodeError):
        return 1
    if args[0] == "health":
        return 0 if healthy(payload) else 1
    if args[0] == "ready-model":
        return 0 if ready_model(payload, args[1]) else 1
    return 0 if absent_model(payload, args[1]) else 1


if __name__ == "__main__":
    sys.exit(main())
