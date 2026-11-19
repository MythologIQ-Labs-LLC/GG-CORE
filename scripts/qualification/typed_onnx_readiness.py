"""Validate live server-owned ONNX typed task readiness from authenticated IPC listing.

A responsive daemon may still be loading or have only one typed model ready.
The packaged acceptance loop must not treat a zero exit code as that evidence.
"""
import json


def typed_models_ready(payload):
    try:
        report = json.loads(payload)
    except (ValueError, UnicodeError) as exc:
        raise ValueError("invalid-model-list-json") from exc
    if not isinstance(report, dict) or not isinstance(report.get("models"), list):
        raise ValueError("invalid-model-list-shape")
    ready = {}
    for model in report["models"]:
        if not isinstance(model, dict):
            raise ValueError("invalid-model-entry")
        if model.get("state") != "ready":
            continue
        handle = model.get("handle_id")
        caps = model.get("capabilities")
        if type(handle) is not int or handle < 1 or not isinstance(caps, list):
            raise ValueError("invalid-ready-model")
        if not all(isinstance(cap, str) for cap in caps):
            raise ValueError("invalid-model-capabilities")
        if handle in ready:
            raise ValueError("duplicate-model-handle")
        ready[handle] = set(caps)
    if len(ready) < 2:
        return False
    return (any("embedding" in caps for caps in ready.values())
            and any("text_classification" in caps for caps in ready.values()))
