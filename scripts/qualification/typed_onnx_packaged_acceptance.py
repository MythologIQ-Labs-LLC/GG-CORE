#!/usr/bin/env python3
"""Offline, read-only packaged ONNX functional acceptance (#171).

Runs ONLY against already staged, operator-approved GG-CORE binaries/models.
Does not download, configure a sandbox, or assert cross-platform acceptance.
All model text, vectors, labels, secrets, local paths and process logs stay
out of the redacted receipt. Auth is supplied to children in the environment.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import secrets
import subprocess
import sys
import tempfile
import time

SHA = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class AcceptanceError(Exception):
    """Expected refusal with a stable, non-sensitive code."""


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as data:
        for chunk in iter(lambda: data.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checked_file(root, name, digest):
    if not isinstance(name, str) or not name or Path(name).is_absolute() or ".." in Path(name).parts:
        raise AcceptanceError("invalid-relative-file")
    if not isinstance(digest, str) or SHA.fullmatch(digest) is None:
        raise AcceptanceError("invalid-expected-sha")
    root = root.resolve(strict=True)
    resolved = (root / name).resolve(strict=True)
    if not resolved.is_relative_to(root) or not resolved.is_file():
        raise AcceptanceError("file-outside-stage")
    if sha256_file(resolved) != digest:
        raise AcceptanceError("file-sha-mismatch")
    return resolved


def finite_vector(reply, dimension=384):
    if not isinstance(reply, dict) or reply.get("kind") != "embedding":
        raise AcceptanceError("embedding-shape")
    values = reply.get("vector")
    if reply.get("dimensions") != dimension or not isinstance(values, list):
        raise AcceptanceError("embedding-dimensions")
    if len(values) != dimension or any(
        not isinstance(x, (int, float)) or isinstance(x, bool) or not math.isfinite(x)
        for x in values
    ):
        raise AcceptanceError("embedding-values")
    return values


def cosine(a, b):
    dot = sum(x * y for x, y in zip(a, b))
    norms = math.sqrt(sum(x * x for x in a) * sum(y * y for y in b))
    if not math.isfinite(norms) or norms == 0:
        raise AcceptanceError("invalid-vector-norm")
    return dot / norms


def expected_classification(reply, label):
    if not isinstance(reply, dict) or reply.get("kind") != "classification":
        raise AcceptanceError("classification-shape")
    confidence = reply.get("confidence")
    if reply.get("label") != label or not isinstance(confidence, (int, float)):
        raise AcceptanceError("classification-label")
    if isinstance(confidence, bool) or not math.isfinite(confidence) or not 0 <= confidence <= 1:
        raise AcceptanceError("classification-confidence")


def invoke(binary, env, args, timeout=90):
    try:
        return subprocess.run(
            [str(binary), *args], env=env, capture_output=True, timeout=timeout,
            check=False, cwd=str(binary.parent),
        )
    except subprocess.TimeoutExpired as exc:
        raise AcceptanceError("command-timeout") from exc


def typed(binary, env, mode, model, prompt):
    result = invoke(binary, env, [mode, "--model", model, "--text", prompt, "--timeout-ms", "60000"])
    if result.returncode != 0:
        raise AcceptanceError(f"{mode}-command-refused")
    try:
        return json.loads(result.stdout)
    except (ValueError, UnicodeError) as exc:
        raise AcceptanceError(f"{mode}-invalid-json") from exc


def required_refusal(result, phrase, name):
    if result.returncode == 0 or phrase.encode() not in result.stderr.lower():
        raise AcceptanceError(f"{name}-refusal-not-proven")


def verify_spec(spec, root):
    if not isinstance(spec, dict) or COMMIT.fullmatch(spec.get("source_commit", "")) is None:
        raise AcceptanceError("invalid-source-commit")
    binary = checked_file(root, spec["binary"], spec["binary_sha256"])
    if not binary.name.startswith("gg-core-cli"):
        raise AcceptanceError("wrong-cli-name")
    ids = set()
    for name in ("embedding", "classification"):
        entry = spec.get(name)
        if not isinstance(entry, dict) or not isinstance(entry.get("id"), str):
            raise AcceptanceError("invalid-model-entry")
        if not re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", entry["id"]) or entry["id"] in ids:
            raise AcceptanceError("invalid-model-id")
        ids.add(entry["id"])
        for key in ("model", "tokenizer", "manifest"):
            checked_file(root, entry[key], entry[key + "_sha256"])
        if not entry.get("revision") or not entry.get("license"):
            raise AcceptanceError("unattributed-model")
    return binary


def run_checks(binary, spec, stage, timeout):
    # A fresh random local endpoint and token bind this run to its daemon.
    # Never weaken resource limits or the OS sandbox through environment overrides.
    env = {k: v for k, v in os.environ.items() if k != "CORE_AUTH_TOKEN" and not k.startswith("GG_CORE_")}
    env["CORE_AUTH_TOKEN"] = secrets.token_hex(32)
    with tempfile.TemporaryDirectory(prefix="gg171-") as temp:
        socket = (r"\\.\pipe\gg171-" + secrets.token_hex(8) if os.name == "nt"
                  else str(Path(temp) / "gg-core.sock"))
        env["GG_CORE_SOCKET_PATH"] = socket
        version = invoke(binary, env, ["version", "--json"], timeout=10)
        if version.returncode:
            raise AcceptanceError("version-command-failed")
        try:
            v = json.loads(version.stdout)
        except ValueError as exc:
            raise AcceptanceError("version-invalid-json") from exc
        if v.get("commit") != spec["source_commit"] or "onnx" not in v.get("features", []):
            raise AcceptanceError("producer-identity-or-feature-mismatch")
        args = [str(binary), "serve"]
        for task in ("embedding", "classification"):
            m = spec[task]
            args.extend(["--model", m["model"], "--model-id", m["id"]])
        daemon = subprocess.Popen(args, cwd=stage, env=env, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + timeout
            while True:
                if daemon.poll() is not None:
                    raise AcceptanceError("daemon-exited-before-ready")
                if time.monotonic() >= deadline:
                    raise AcceptanceError("daemon-startup-timeout")
                try:
                    probe = invoke(binary, env, ["models", "list", "--json"], timeout=5)
                    if probe.returncode == 0:
                        break
                except AcceptanceError:
                    pass
                time.sleep(1)
            embed, classifier = spec["embedding"]["id"], spec["classification"]["id"]
            a = finite_vector(typed(binary, env, "embed", embed, "A man eats his lunch."))
            b = finite_vector(typed(binary, env, "embed", embed, "A person is eating a meal."))
            c = finite_vector(typed(binary, env, "embed", embed, "A rocket enters orbit."))
            if cosine(a, b) <= cosine(a, c):
                raise AcceptanceError("embedding-semantic-order")
            expected_classification(typed(binary, env, "classify", classifier,
                                          "I loved this movie."), "POSITIVE")
            expected_classification(typed(binary, env, "classify", classifier,
                                          "I hated this movie."), "NEGATIVE")
            refused = invoke(binary, env, ["classify", "--model", embed,
                                           "--text", "Example."], timeout=90)
            required_refusal(refused, "not supported", "wrong-task")
            denied_env = dict(env, CORE_AUTH_TOKEN=secrets.token_hex(32))
            denied = invoke(binary, denied_env, ["embed", "--model", embed,
                                                 "--text", "Example."], timeout=30)
            required_refusal(denied, "authentication failed", "wrong-auth")
            finite_vector(typed(binary, env, "embed", embed, "A person is eating a meal."))
        finally:
            daemon.terminate()
            try:
                daemon.wait(timeout=10)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=10)
    return {"typed_embedding": "pass", "similarity_order": "pass",
            "typed_classification": "pass", "wrong_task": "pass", "wrong_auth": "pass"}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--startup-timeout", type=int, default=180)
    args = parser.parse_args(argv)
    receipt = {"schema": 1, "status": "FAIL", "scope": "offline_packaged_typed_onnx_functional",
               "not_proven": ["sandbox_containment", "cross_platform", "long_run",
                              "memory_reclamation", "release_qualification"]}
    try:
        if not 1 <= args.startup_timeout <= 900:
            raise AcceptanceError("invalid-startup-timeout")
        spec = json.loads(args.spec.read_text(encoding="utf-8"))
        binary = verify_spec(spec, args.stage)
        receipt.update(source_commit=spec["source_commit"], binary_sha256=spec["binary_sha256"],
                       model_sha256={t: spec[t]["model_sha256"] for t in ("embedding", "classification")},
                       platform=platform.system())
        receipt["checks"] = run_checks(binary, spec, args.stage.resolve(), args.startup_timeout)
        receipt["status"] = "PASS"
    except (AcceptanceError, ValueError, KeyError, OSError, TypeError) as exc:
        receipt["error_code"] = (str(exc) if isinstance(exc, AcceptanceError)
                                 else "invalid-input-or-environment")
    # Operator chooses a protected receipt directory. No local paths, prompts,
    # model output, classification text, auth tokens, or raw stderr are emitted.
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(f"{receipt['status']}: typed ONNX packaged functional acceptance")
    return 0 if receipt["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
