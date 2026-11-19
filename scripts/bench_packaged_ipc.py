#!/usr/bin/env python3
"""Measure authenticated packaged GG-CORE IPC without altering runtime state."""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import platform
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone

from bench_packaged_ipc_helpers import (
    IpcClient, ProtocolFailure, file_sha256, verify_packaged_binary,
    sample, quantile, summary,
)

def run_wave(endpoint: str, token: str, model: str, prompt: str,
             concurrency: int, stream: bool, max_tokens: int, timeout_ms: int):
    barrier = threading.Barrier(concurrency) if concurrency > 1 else None
    def worker(i):
        with IpcClient(endpoint, timeout_ms / 1000 + 5) as client:
            client.authenticate(token)
            return sample(client, model, prompt, max_tokens, timeout_ms, stream,
                          i + 1, barrier)
    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        return list(pool.map(worker, range(concurrency)))


def checked_inputs(args):
    token = os.environ.get(args.auth_env)
    if not token or len(token.encode("utf-8")) < 16:
        raise ValueError("a private authentication token of >=16 bytes is required")
    prompt_bytes = args.prompt_file.read_bytes()
    prompt = prompt_bytes.decode("utf-8")
    if not prompt.strip():
        raise ValueError("prompt fixture is empty")
    with args.model_file.open("rb") as model_stream:
        if model_stream.read(4) != b"GGUF":
            raise ValueError("model fixture is not a real GGUF file (possible Git LFS pointer)")
    return {
        "token": token, "prompt": prompt, "prompt_bytes": prompt_bytes,
        "model_hash": file_sha256(args.model_file),
        "binary_hash": file_sha256(args.daemon_binary),
        "binary_version": verify_packaged_binary(args.daemon_binary, args.producer_sha),
    }


def authenticated_warmup(args, token):
    with IpcClient(args.socket, args.timeout_ms / 1000 + 5) as client:
        handshake_ms = client.authenticate(token)
        start = time.perf_counter()
        client.send({"type": "warmup_request", "model_id": args.model_id, "tokens": 1})
        warm = client.receive(time.monotonic() + args.timeout_ms / 1000 + 5)
        warmup_ms = (time.perf_counter() - start) * 1000
    if (warm.get("type") != "warmup_response" or warm.get("success") is not True
            or warm.get("model_id") != args.model_id or warm.get("error") is not None
            or type(warm.get("elapsed_ms")) is not int or warm["elapsed_ms"] < 0):
        raise ProtocolFailure("authenticated model warmup failed or malformed response")
    return round(handshake_ms, 3), round(warmup_ms, 3), warm["elapsed_ms"]


def measure_cohorts(args, token, prompt):
    records = []
    for concurrency in args.concurrent:
        samples = []
        for _ in range(args.waves):
            samples.extend(run_wave(args.socket, token, args.model_id, prompt,
                                    concurrency, args.stream, args.max_tokens, args.timeout_ms))
        records.append({"concurrency": concurrency, "stats": summary(samples),
                        "samples": samples})
    return records


def run_benchmark(args):
    identity = checked_inputs(args)
    handshake_ms, warmup_ms, daemon_warmup_ms = authenticated_warmup(args, identity["token"])
    records = measure_cohorts(args, identity["token"], identity["prompt"])
    return {
        "schema_version": 2,
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "producer_sha": args.producer_sha,
        "model_sha256": identity["model_hash"],
        "daemon_sha256": identity["binary_hash"],
        "packaged_binary_version": identity["binary_version"],
        "prompt_sha256": hashlib.sha256(identity["prompt_bytes"]).hexdigest(),
        "model_id": args.model_id,
        "host": {"platform": platform.platform(), "machine": platform.machine(),
                 "python": platform.python_version(), "cpu_count": os.cpu_count()},
        "requested": {"stream": args.stream, "max_tokens": args.max_tokens,
                      "timeout_ms": args.timeout_ms, "concurrency": args.concurrent,
                      "waves": args.waves, "auth": True,
                      "security_and_sandbox_configuration": "not independently established by benchmark"},
        "handshake_ms": handshake_ms,
        "warmup_request_ms": warmup_ms,
        "daemon_reported_warmup_ms": daemon_warmup_ms,
        "results": records,
        "limitations": [
            "The benchmark rejects raw token IDs; it does not independently prove PII/injection decision parity.",
            "The supplied binary's version and source commit are executed and checked; the running daemon's PID/image and loaded model identity are NOT independently attested.",
            "First-sanitized-text latency is not equivalent to model-native TTFT.",
            "Measured latency starts at request send and excludes connection/authentication, model loading, and model download.",
            "This run does not independently verify sandbox state, security-decision parity, RSS, or token throughput.",
            "This is a measurement artifact, not a release performance gate or a comparative speed claim."
        ]
    }


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--socket", required=True, help="Already-running GG-CORE socket or Windows named pipe")
    p.add_argument("--model-id", required=True)
    p.add_argument("--model-file", type=Path, required=True, help="Exact GGUF bytes for SHA256 provenance")
    p.add_argument("--daemon-binary", type=Path, required=True, help="Packaged GG-CORE executable for SHA256 provenance")
    p.add_argument("--producer-sha", required=True, help="40-char immutable GG-CORE source commit")
    p.add_argument("--prompt-file", type=Path, required=True)
    p.add_argument("--auth-env", default="CORE_AUTH_TOKEN", help="Environment variable name only; never pass secrets via argv")
    p.add_argument("--stream", action="store_true")
    p.add_argument("--max-tokens", type=int, default=64)
    p.add_argument("--timeout-ms", type=int, default=120000)
    p.add_argument("--concurrent", type=int, nargs="+", default=[1, 4, 8])
    p.add_argument("--waves", type=int, default=10)
    p.add_argument("--output", type=Path, required=True, help="JSON report, no prompt/response text stored")
    args = p.parse_args(argv)
    if len(args.producer_sha) != 40 or any(c not in "0123456789abcdefABCDEF" for c in args.producer_sha):
        p.error("--producer-sha must be a complete 40-character hexadecimal commit")
    if args.max_tokens <= 0 or args.timeout_ms <= 0 or args.waves <= 0 or not args.concurrent or any(x < 1 or x > 64 for x in args.concurrent):
        p.error("max tokens, timeout, waves and concurrency must be positive; concurrency <=64")
    if len(set(args.concurrent)) != len(args.concurrent):
        p.error("duplicate --concurrent values are not meaningful")
    return args


def main(argv=None):
    args = parse_args(argv)
    try:
        # Refuse the expensive run up front for an already-used evidence path.
        # os.link below is the final no-clobber guard against races.
        if args.output.exists() or args.output.is_symlink():
            raise FileExistsError("benchmark report already exists; use a new output path")
        report = run_benchmark(args)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        # A prior qualified report must not be silently overwritten by new
        # measurements, or by a failed/retried run. Publish an exclusive
        # hard link from a fully written, private temporary file. This is
        # atomic on the same filesystem, fails if the report path exists,
        # and leaves no partial success report on an interrupted write.
        temp_path = None
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8", dir=args.output.parent,
                prefix=".gg-core-bench-", suffix=".tmp", delete=False
            ) as temp:
                temp_path = Path(temp.name)
                json.dump(report, temp, indent=2, sort_keys=True)
                temp.write("\n")
                temp.flush()
                os.fsync(temp.fileno())
            os.link(temp_path, args.output)
        finally:
            if temp_path is not None:
                temp_path.unlink(missing_ok=True)
        print(f"PASS: benchmark finished; report written to {args.output}")
        print("Metrics: request latency and first sanitized text; NOT native TTFT/tokens per second.")
        return 0
    except (OSError, ValueError, ProtocolFailure, TimeoutError, EOFError) as error:
        print(f"FAIL: benchmark did not qualify: {type(error).__name__}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())