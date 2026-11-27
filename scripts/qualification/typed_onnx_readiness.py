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
        if "capabilities" not in model:
            # An unauthenticated listing omits capabilities by design; this is
            # not evidence that the typed models are unready (#171).
            raise ValueError("ready-model-capabilities-absent")
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


def wait_typed_ready(invoke, binary, env, daemon, timeout, sleep=None, clock=None):
    """Poll the authenticated listing until both typed models are ready.

    Malformed or capability-free listings are retried, but a timeout reports
    the last stable reason instead of a generic startup timeout. Returns
    ``None`` when ready, otherwise a stable, non-sensitive error code.
    """
    import time
    sleep = sleep or time.sleep
    clock = clock or time.monotonic
    deadline, last = clock() + timeout, None
    while True:
        if daemon.poll() is not None:
            return "daemon-exited-before-ready"
        if clock() >= deadline:
            return "daemon-startup-timeout" if last is None else f"readiness-timeout-{last}"
        probe = invoke(binary, env, ["models", "list", "--json"], timeout=5)
        if probe.returncode != 0:
            last = "models-list-refused"
        else:
            try:
                if typed_models_ready(probe.stdout):
                    return None
                last = "typed-models-not-ready"
            except UnicodeError:
                last = "models-list-invalid"
            except ValueError as exc:
                last = str(exc)
        sleep(1)
