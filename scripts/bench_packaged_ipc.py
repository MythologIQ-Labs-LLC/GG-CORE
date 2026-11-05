#!/usr/bin/env python3
"""Repeatable latency measurement through a running GG-CORE packaged IPC daemon.

Non-publishing, read-only benchmark. Auth is required. No model lifecycle changes.
Does NOT report native TTFT or tokens/s: streaming output is sanitized text.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import socket
import tempfile
import struct
import sys
import threading
import time
from datetime import datetime, timezone

MAX_FRAME_BYTES = 16 * 1024 * 1024


class ProtocolFailure(Exception):
    pass


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


class IpcClient:
    def __init__(self, endpoint: str, timeout_s: float):
        self.endpoint, self.timeout_s = endpoint, timeout_s
        self.pipe = None
        self.sock = None
        self.peek = None

    def __enter__(self):
        if os.name == "nt":
            from ctypes import wintypes
            import msvcrt

            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            wait = kernel32.WaitNamedPipeW
            wait.argtypes = (wintypes.LPCWSTR, wintypes.DWORD)
            wait.restype = wintypes.BOOL
            if not wait(self.endpoint, min(int(self.timeout_s * 1000), 0xFFFFFFFE)):
                raise TimeoutError("named pipe is unavailable before deadline")
            self.pipe = open(self.endpoint, "r+b", buffering=0)
            self.handle = msvcrt.get_osfhandle(self.pipe.fileno())
            self.peek = kernel32.PeekNamedPipe
            self.peek.argtypes = (
                wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p,
            )
            self.peek.restype = wintypes.BOOL
        else:
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.settimeout(self.timeout_s)
            try:
                self.sock.connect(self.endpoint)
            except BaseException:
                self.sock.close()
                self.sock = None
                raise
        return self

    def __exit__(self, *_):
        if self.pipe:
            self.pipe.close()
        if self.sock:
            self.sock.close()

    def send(self, message: dict):
        b = json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
        if len(b) == 0 or len(b) > MAX_FRAME_BYTES:
            raise ProtocolFailure("invalid outbound IPC frame size")
        frame = struct.pack("<I", len(b)) + b
        if self.pipe:
            self.pipe.write(frame)
        else:
            self.sock.sendall(frame)

    def read_exact(self, n: int, deadline: float) -> bytes:
        out = bytearray()
        while len(out) < n:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("IPC response deadline expired")
            if self.pipe:
                available = ctypes.c_ulong()
                if not self.peek(self.handle, None, 0, None, ctypes.byref(available), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                if available.value == 0:
                    time.sleep(min(0.01, remaining))
                    continue
                chunk = self.pipe.read(min(n - len(out), available.value))
            else:
                self.sock.settimeout(remaining)
                chunk = self.sock.recv(n - len(out))
            if not chunk:
                raise EOFError("daemon closed IPC connection")
            out.extend(chunk)
        return bytes(out)

    def receive(self, deadline: float) -> dict:
        length = struct.unpack("<I", self.read_exact(4, deadline))[0]
        if not (0 < length <= MAX_FRAME_BYTES):
            raise ProtocolFailure("invalid inbound IPC frame size")
        result = json.loads(self.read_exact(length, deadline).decode("utf-8"))
        if not isinstance(result, dict) or not isinstance(result.get("type"), str):
            raise ProtocolFailure("invalid IPC response object")
        return result

    def authenticate(self, token: str) -> float:
        start = time.perf_counter()
        self.send({"type": "handshake", "token": token, "protocol_version": "V1"})
        response = self.receive(time.monotonic() + self.timeout_s)
        if (response.get("type") != "handshake_ack"
                or response.get("protocol_version") != "V1"
                or not isinstance(response.get("session_id"), str)
                or not response["session_id"]):
            raise ProtocolFailure("authenticated V1 handshake failed or missing session")
        return (time.perf_counter() - start) * 1000


def sample(client: IpcClient, model: str, prompt: str, max_tokens: int,
           timeout_ms: int, stream: bool, request_id: int, barrier: threading.Barrier | None):
    params = {"stream": stream, "max_tokens": max_tokens, "timeout_ms": timeout_ms}
    payload = {"type": "inference_request", "request_id": request_id,
               "model_id": model, "prompt": prompt, "parameters": params}
    if barrier:
        barrier.wait(timeout=max(5, timeout_ms / 1000))
    start = time.perf_counter()
    client.send(payload)
    deadline = time.monotonic() + timeout_ms / 1000 + 5
    first_text_ms = None
    frames, text_chars = 0, 0
    while True:
        response = client.receive(deadline)
        if type(response.get("request_id")) is not int or response["request_id"] != request_id:
            raise ProtocolFailure("request ID mismatch")
        if response["type"] == "inference_response" and not stream:
            if response.get("error") is not None or response.get("finished") is not True:
                raise ProtocolFailure("nonstream inference failed or incomplete")
            result = response.get("output")
            if not isinstance(result, str) or not result:
                raise ProtocolFailure("nonstream inference returned no text")
            generated = response.get("tokens_generated")
            if type(generated) is not int or generated < 0:
                raise ProtocolFailure("nonstream inference has invalid token count")
            text_chars = len(result)
            break
        if response["type"] == "stream_chunk" and stream:
            if response.get("error") is not None:
                raise ProtocolFailure("stream returned terminal error")
            if type(response.get("is_final")) is not bool:
                raise ProtocolFailure("stream frame has invalid terminal flag")
            if type(response.get("token")) is not int or response["token"] != 0:
                raise ProtocolFailure("raw token ID exposed through sanitized IPC path")
            part = response.get("text")
            if part is not None:
                if not isinstance(part, str):
                    raise ProtocolFailure("stream text is not a string")
                if part and first_text_ms is None:
                    first_text_ms = (time.perf_counter() - start) * 1000
                text_chars += len(part)
            if response.get("is_final"):
                break
            frames += 1
            continue
        raise ProtocolFailure("unexpected IPC message type")
    elapsed = (time.perf_counter() - start) * 1000
    if stream and not text_chars:
        raise ProtocolFailure("stream returned no sanitized output")
    return {"duration_ms": elapsed, "first_sanitized_text_ms": first_text_ms,
            "visible_chars": text_chars, "stream_data_frames": frames}


def quantile(values: list[float], fraction: float) -> float:
    if not values:
        raise ValueError("cannot compute quantile of empty data")
    a = sorted(values)
    pos = (len(a) - 1) * fraction
    low, high = int(pos), min(int(pos) + 1, len(a) - 1)
    return a[low] + (a[high] - a[low]) * (pos - low)


def summary(samples: list[dict]):
    if not samples:
        raise ValueError("no successful IPC samples")
    durations = [x["duration_ms"] for x in samples]
    first = [x["first_sanitized_text_ms"] for x in samples
             if x["first_sanitized_text_ms"] is not None]
    def dist(xs):
        if not xs:
            return None
        # Avoid printing a precise-looking tail percentile based on 1-10
        # samples. These are descriptive sample percentiles, NOT SLA bounds.
        n = len(xs)
        return {"p50": round(quantile(xs, .50), 3),
                "p95": round(quantile(xs, .95), 3) if n >= 20 else None,
                "p99": round(quantile(xs, .99), 3) if n >= 100 else None}
    return {"count": len(samples), "duration_ms": dist(durations),
            "first_sanitized_text_ms": dist(first),
            "note": "Sample p95 requires >=20 observations; p99 >=100. These are not SLA bounds. First sanitized text is not native TTFT; character counts are not token counts."}


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


def run_benchmark(args):
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
    model_hash = file_sha256(args.model_file)
    binary_hash = file_sha256(args.daemon_binary)
    with IpcClient(args.socket, args.timeout_ms / 1000 + 5) as client:
        handshake_ms = client.authenticate(token)
        warmup_start = time.perf_counter()
        client.send({"type": "warmup_request", "model_id": args.model_id, "tokens": 1})
        warm = client.receive(time.monotonic() + args.timeout_ms / 1000 + 5)
        warmup_request_ms = (time.perf_counter() - warmup_start) * 1000
        if (warm.get("type") != "warmup_response" or warm.get("success") is not True
                or warm.get("model_id") != args.model_id or warm.get("error") is not None
                or type(warm.get("elapsed_ms")) is not int or warm["elapsed_ms"] < 0):
            raise ProtocolFailure("authenticated model warmup failed or malformed response")
    records = []
    for concurrency in args.concurrent:
        samples = []
        for _ in range(args.waves):
            samples.extend(run_wave(args.socket, token, args.model_id, prompt,
                                    concurrency, args.stream, args.max_tokens, args.timeout_ms))
        records.append({"concurrency": concurrency, "stats": summary(samples),
                        "samples": samples})
    return {
        "schema_version": 2,
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "producer_sha": args.producer_sha,
        "model_sha256": model_hash,
        "daemon_sha256": binary_hash,
        "prompt_sha256": hashlib.sha256(prompt_bytes).hexdigest(),
        "model_id": args.model_id,
        "host": {"platform": platform.platform(), "machine": platform.machine(),
                 "python": platform.python_version(), "cpu_count": os.cpu_count()},
        "requested": {"stream": args.stream, "max_tokens": args.max_tokens,
                      "timeout_ms": args.timeout_ms, "concurrency": args.concurrent,
                      "waves": args.waves, "auth": True,
                      "security_and_sandbox_configuration": "not independently established by benchmark"},
        "handshake_ms": round(handshake_ms, 3),
        "warmup_request_ms": round(warmup_request_ms, 3),
        "daemon_reported_warmup_ms": warm["elapsed_ms"],
        "results": records,
        "limitations": [
            "The benchmark rejects raw token IDs; it does not independently prove PII/injection decision parity.",
            "Source SHA and model-file hash are caller-supplied associations; the running daemon's binary/model identity is not independently attested.",
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