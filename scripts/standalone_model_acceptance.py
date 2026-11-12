#!/usr/bin/env python3
"""Offline, model-agnostic acceptance of the packaged GG-CORE daemon and CLI.

Does not download weights, open a network socket, modify the model directory,
or accept an insecure daemon. Qualifies one pinned fixture per invocation.
See docs/STANDALONE_MODEL_ACCEPTANCE.md for trust/coverage limits.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import signal
import subprocess
import sys
import tempfile
import time
import uuid

TASK_FEATURE = {"generation": "gguf", "embedding": "onnx", "classification": "onnx"}
TASK_CLI = {"embedding": "embed", "classification": "classify"}
TASK_CAPABILITY = {
    "generation": "text_generation", "embedding": "embedding", "classification": "text_classification"
}


class AcceptanceFailure(Exception):
    pass


def require(ok, message):
    if not ok:
        raise AcceptanceFailure(message)


def sha256_file(path):
    sha = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            sha.update(block)
    return sha.hexdigest()


def fixture_path(base, model_relative):
    """Require existing regular file under base/models, without symlink traversal."""
    rel = Path(model_relative)
    require(not rel.is_absolute() and rel.parts[:1] == ("models",),
            "fixture path must be relative to base/models")
    require(".." not in rel.parts, "fixture must not contain parent traversal")
    base = base.resolve(strict=True)
    model_root = base / "models"
    require(model_root.is_dir() and not model_root.is_symlink(), "base/models must be a real directory")
    current = base
    for part in rel.parts:
        current = current / part
        require(not current.is_symlink(), "model path may not include symlinks")
    path = current.resolve(strict=True)
    require(path.is_file() and path.is_relative_to(model_root.resolve()),
            "fixture must be a regular file under base/models")
    return path


def verify_version(doc, expected_commit, task):
    require(isinstance(doc, dict), "version --json must be an object")
    require(doc.get("name") == "GG-CORE", "wrong binary product identity")
    commit = doc.get("commit")
    require(isinstance(commit, str) and re.fullmatch(r"[0-9a-fA-F]{7,40}", commit),
            "missing usable build commit provenance")
    require(len(expected_commit) == 40 and re.fullmatch(r"[0-9a-fA-F]{40}", expected_commit),
            "expected commit must be a full 40-character SHA-1")
    require(expected_commit.lower().startswith(commit.lower()) or
            commit.lower().startswith(expected_commit.lower()),
            "binary build commit does not match the pinned source commit")
    features = doc.get("features")
    require(isinstance(features, list) and all(isinstance(x, str) for x in features),
            "version features must be an array of strings")
    require(TASK_FEATURE[task] in features, "binary lacks the required compiled backend")
    require(isinstance(doc.get("target"), str) and "-" in doc["target"],
            "missing target provenance")
    require(isinstance(doc.get("protocol_version"), str), "missing protocol version")
    return {"commit": commit, "version": doc.get("version"),
            "target": doc["target"], "features": features,
            "protocol_version": doc["protocol_version"]}


def validate_typed(result, task):
    require(isinstance(result, dict), "typed output must be JSON object")
    kind = result.get("kind")
    if task == "embedding":
        require(kind == "embedding", "typed result is not an embedding")
        dims, vec = result.get("dimensions"), result.get("vector")
        require(type(dims) is int and 0 < dims <= 1_000_000, "embedding dimensions invalid")
        require(isinstance(vec, list) and len(vec) == dims, "embedding vector length mismatch")
        require(all(type(x) in (int, float) and math.isfinite(x) for x in vec),
                "embedding contains nonfinite/non-numeric values")
        return {"kind": kind, "dimensions": dims}
    require(kind == "classification", "typed result is not a classification")
    label, confidence, labels = result.get("label"), result.get("confidence"), result.get("all_labels")
    require(isinstance(label, str) and bool(label), "classification label missing")
    require(type(confidence) in (float, int) and math.isfinite(confidence) and 0 <= confidence <= 1,
            "classification confidence invalid")
    require(isinstance(labels, list) and bool(labels), "classification scores missing")
    for item in labels:
        require(isinstance(item, dict) and isinstance(item.get("label"), str),
                "invalid score label")
        score = item.get("confidence")
        require(type(score) in (int, float) and math.isfinite(score) and 0 <= score <= 1,
                "invalid score confidence")
    return {"kind": kind, "label": label, "label_count": len(labels)}


def clean_environment(socket_address, token, n_ctx):
    env = os.environ.copy()
    for key in list(env):
        if key.startswith("GG_CORE_") or key == "CORE_AUTH_TOKEN":
            del env[key]
    env.update({
        "CORE_AUTH_TOKEN": token,
        "GG_CORE_SOCKET_PATH": socket_address,
        "GG_CORE_N_CTX": str(n_ctx),
        "GG_CORE_MAX_CONTEXT": str(n_ctx),
        "GG_CORE_MAX_CONTEXT_TOKENS": str(n_ctx),
    })
    return env


def run_cli(binary, base, env, argv, *, timeout=30, success=True):
    try:
        result = subprocess.run([str(binary), *argv], cwd=base, env=env,
                                capture_output=True, text=True, timeout=timeout,
                                encoding="utf-8", errors="replace", check=False)
    except subprocess.TimeoutExpired as exc:
        raise AcceptanceFailure(f"CLI command timed out: {argv[0]}") from exc
    if success is True:
        require(result.returncode == 0, f"CLI command failed: {argv[0]} (exit {result.returncode})")
    elif success is False:
        require(result.returncode != 0, f"negative check unexpectedly accepted: {argv[0]}")
    return result.stdout


def await_ready(binary, base, env, daemon, seconds):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        require(daemon.poll() is None, "daemon exited during startup")
        try:
            run_cli(binary, base, env, ["ready"], timeout=3)
            return
        except AcceptanceFailure:
            time.sleep(0.25)
    raise AcceptanceFailure("daemon failed to become ready within startup deadline")


def check_list(binary, base, env, model_id):
    raw = run_cli(binary, base, env, ["models", "list", "--json"])
    try:
        doc = json.loads(raw)
    except ValueError as exc:
        raise AcceptanceFailure("models list did not emit JSON") from exc
    models = doc.get("models") if isinstance(doc, dict) else None
    require(isinstance(models, list), "models list missing models array")
    matched = next((m for m in models if isinstance(m, dict) and m.get("name") == model_id), None)
    require(matched is not None, "preloaded model absent from model list")
    return matched


def infer(binary, base, env, model_id, prompt, timeout, template, *, stream=False):
    cmd = ["infer", "--model", model_id, "--prompt", prompt, "--max-tokens", "32",
           "--timeout-ms", str(timeout * 1000)]
    if template:
        cmd.extend(["--chat-template", template])
    if stream:
        cmd.append("--stream")
    output = run_cli(binary, base, env, cmd, timeout=timeout + 10).strip()
    require(bool(output), "generation succeeded but returned empty output")
    return {"chars": len(output), "sha256": hashlib.sha256(output.encode()).hexdigest()}


def check_task(binary, base, env, args):
    if args.task == "generation":
        a = infer(binary, base, env, args.model_id, args.prompt, args.inference_timeout,
                  args.chat_template)
        b = infer(binary, base, env, args.model_id, args.prompt, args.inference_timeout,
                  args.chat_template, stream=True)
        return {"generation": a, "streaming": b}
    out = run_cli(binary, base, env,
                  [TASK_CLI[args.task], "--model", args.model_id, "--text", args.text,
                   "--timeout-ms", str(args.inference_timeout * 1000)],
                  timeout=args.inference_timeout + 10)
    try:
        value = json.loads(out)
    except ValueError as exc:
        raise AcceptanceFailure("typed command did not emit JSON") from exc
    return validate_typed(value, args.task)


def stop_daemon(daemon):
    if daemon.poll() is not None:
        return
    if os.name != "nt":
        daemon.send_signal(signal.SIGINT)
    else:
        daemon.terminate()
    try:
        daemon.wait(timeout=12)
    except subprocess.TimeoutExpired:
        daemon.kill()
        daemon.wait(timeout=5)


def execute(args):
    base = args.base.resolve(strict=True)
    binary = args.cli.resolve(strict=True)
    require(binary.is_file(), "CLI executable not found")
    require(re.fullmatch(r"[a-fA-F0-9]{64}", args.model_sha256), "model hash must be 64 hex chars")
    require(args.model_id and len(args.model_id) <= 100, "invalid model id")
    fixture = fixture_path(base, args.model)
    actual = sha256_file(fixture)
    require(actual.lower() == args.model_sha256.lower(), "model fixture SHA-256 mismatch")
    binary_hash = sha256_file(binary)
    require(re.fullmatch(r"[a-fA-F0-9]{64}", args.cli_sha256), "CLI hash must be 64 hex chars")
    require(binary_hash.lower() == args.cli_sha256.lower(), "packaged CLI SHA-256 mismatch")
    with tempfile.TemporaryDirectory(prefix="ggc-accept-") as temp:
        if os.name == "nt":
            address = "\\\\.\\pipe\\ggc-accept-" + uuid.uuid4().hex
        else:
            # Unix socket paths have a small fixed length limit; don't place them in HOME.
            address = "/tmp/ggc-" + uuid.uuid4().hex[:12] + ".sock"
        env = clean_environment(address, secrets.token_hex(32), args.n_ctx)
        version_out = run_cli(binary, base, env, ["version", "--json"])
        try:
            version_doc = json.loads(version_out)
        except ValueError as exc:
            raise AcceptanceFailure("version --json did not emit JSON") from exc
        version = verify_version(version_doc, args.commit, args.task)
        run_cli(binary, base, env, ["config", "validate"])
        with open(Path(temp) / "daemon.log", "wb") as logfile:
            daemon = subprocess.Popen(
                [str(binary), "serve", "--model", args.model, "--model-id", args.model_id],
                cwd=base, env=env, stdout=logfile, stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL)
            try:
                await_ready(binary, base, env, daemon, args.startup_timeout)
                run_cli(binary, base, env, ["live"])
                status = run_cli(binary, base, env, ["status", "--json"])
                require(isinstance(json.loads(status), dict), "status is not JSON object")
                loaded = check_list(binary, base, env, args.model_id)
                caps = loaded.get("capabilities", [])
                require(isinstance(caps, list), "model capabilities not a list")
                # A missing capability list on older runtimes is not a positive capability claim.
                if caps:
                    require(TASK_CAPABILITY[args.task] in caps,
                            "model advertises a different task capability")
                outcome = check_task(binary, base, env, args)
                wrong = dict(env, CORE_AUTH_TOKEN=secrets.token_hex(32))
                if args.task == "generation":
                    neg = ["infer", "--model", args.model_id, "--prompt", "hello", "--max-tokens", "2"]
                else:
                    neg = [TASK_CLI[args.task], "--model", args.model_id, "--text", "hello"]
                run_cli(binary, base, wrong, neg, timeout=10, success=False)
                run_cli(binary, base, env,
                        ["models", "unload", args.model_id])
                run_cli(binary, base, env, ["ready"], success=False)
                run_cli(binary, base, env,
                        ["models", "load", args.model, "--id", args.model_id],
                        timeout=args.startup_timeout)
                run_cli(binary, base, env, ["ready"])
                check_list(binary, base, env, args.model_id)
                recovery = check_task(binary, base, env, args)
                require(daemon.poll() is None, "daemon died during acceptance checks")
            finally:
                stop_daemon(daemon)
    return {
        "result": "PASS", "qualified_scope": "one pinned fixture, local packaged CLI, no GUI",
        "source_commit": version["commit"], "cli_sha256": binary_hash,
        "model_sha256": actual, "model_id": args.model_id,
        "task": args.task, "version": version,
        "results": {"initial": outcome, "after_unload_reload": recovery},
        "checks": ["version", "config_validate", "startup_ready", "liveness", "status_json",
                   "model_list", "task_execution", "wrong_token_refused", "unload_not_ready",
                   "reload_ready", "recovery", "daemon_survived"],
        "limits": ["not an OS sandbox audit", "not a throughput or quality benchmark",
                   "not a native cancellation/quiescence test", "one fixture on this host only"]
    }


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cli", required=True, type=Path, help="packaged gg-core-cli executable")
    p.add_argument("--base", required=True, type=Path, help="directory containing models/")
    p.add_argument("--model", required=True, help="relative models/<file> path")
    p.add_argument("--model-sha256", required=True, help="known-good fixture digest")
    p.add_argument("--cli-sha256", required=True, help="expected packaged binary digest")
    p.add_argument("--model-id", default="acceptance-model")
    p.add_argument("--commit", required=True, help="expected full 40-hex source SHA")
    p.add_argument("--task", choices=TASK_FEATURE, required=True)
    p.add_argument("--n-ctx", type=int, default=4096)
    p.add_argument("--startup-timeout", type=int, default=120)
    p.add_argument("--inference-timeout", type=int, default=120)
    p.add_argument("--chat-template", choices=["qwen2.5"], default=None,
                   help="generation only; raw mode is default")
    p.add_argument("--prompt", default="Briefly describe a quiet garden.",
                   help="generation prompt (not logged or included in receipt)")
    p.add_argument("--text", default="The film was enjoyable.",
                   help="embedding/classification input (not logged)")
    p.add_argument("--output", type=Path, help="optional JSON evidence report path")
    args = p.parse_args(argv)
    p.error("--chat-template applies to generation only") if args.chat_template and args.task != "generation" else None
    p.error("--n-ctx must be >=128") if args.n_ctx < 128 else None
    p.error("timeouts must be positive") if args.startup_timeout <= 0 or args.inference_timeout <= 0 else None
    return args


def main(argv=None):
    args = parse_args(argv)
    try:
        evidence = execute(args)
    except (AcceptanceFailure, OSError, ValueError, json.JSONDecodeError) as exc:
        # Avoid echoing generated text, prompt contents, daemon logs or auth secrets.
        print("FAIL: standalone model acceptance: " + type(exc).__name__, file=sys.stderr)
        return 1
    if args.output:
        args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("PASS: standalone GG-CORE model acceptance: " + args.task + " (pinned binary and model)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
