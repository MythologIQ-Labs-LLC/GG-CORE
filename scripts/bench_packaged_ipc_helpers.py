#!/usr/bin/env python3
"""Repeatable latency measurement through a running GG-CORE packaged IPC daemon.

Non-publishing, read-only benchmark. Auth is required. No model lifecycle changes.
Does NOT report native TTFT or tokens/s: streaming output is sanitized text.
"""
from __future__ import annotations

import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import socket
import struct
import threading
import time

MAX_FRAME_BYTES = 16 * 1024 * 1024


class ProtocolFailure(Exception):
    pass


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def verify_packaged_binary(binary: Path, expected_commit: str) -> dict:
    """Bind the supplied executable bytes to its compile-time source identity.

    This intentionally does NOT attest the already-running daemon process.
    The operator must independently bind its PID/image and loaded model.
    """
    try:
        result = subprocess.run(
            [str(binary.resolve(strict=True)), "version", "--json"],
            cwd=str(binary.parent), capture_output=True, timeout=10, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ProtocolFailure("packaged binary version execution failed") from exc
    if result.returncode != 0:
        raise ProtocolFailure("packaged binary version command refused")
    try:
        info = json.loads(result.stdout)
    except (ValueError, UnicodeError) as exc:
        raise ProtocolFailure("packaged binary version is not JSON") from exc
    if not isinstance(info, dict) or info.get("name") != "GG-CORE":
        raise ProtocolFailure("unexpected packaged binary identity")
    if info.get("commit") != expected_commit:
        raise ProtocolFailure("packaged binary source commit mismatch")
    if info.get("protocol_version") != "V1":
        raise ProtocolFailure("packaged binary protocol mismatch")
    if not isinstance(info.get("features"), list) or "gguf" not in info["features"]:
        raise ProtocolFailure("packaged binary lacks GGUF inference feature")
    if not isinstance(info.get("version"), str) or not info["version"]:
        raise ProtocolFailure("packaged binary version absent")
    return {"version": info["version"], "commit": info["commit"],
            "protocol_version": info["protocol_version"]}


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


def checked_nonstream_reply(response):
    if response.get("error") is not None or response.get("finished") is not True:
        raise ProtocolFailure("nonstream inference failed or incomplete")
    output = response.get("output")
    if not isinstance(output, str) or not output:
        raise ProtocolFailure("nonstream inference returned no text")
    generated = response.get("tokens_generated")
    if type(generated) is not int or generated < 0:
        raise ProtocolFailure("nonstream inference has invalid token count")
    return len(output)


def checked_stream_reply(response):
    if response.get("error") is not None:
        raise ProtocolFailure("stream returned terminal error")
    if type(response.get("is_final")) is not bool:
        raise ProtocolFailure("stream frame has invalid terminal flag")
    if type(response.get("token")) is not int or response["token"] != 0:
        raise ProtocolFailure("raw token ID exposed through sanitized IPC path")
    part = response.get("text")
    if part is not None and not isinstance(part, str):
        raise ProtocolFailure("stream text is not a string")
    return response["is_final"], part or ""


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
            text_chars = checked_nonstream_reply(response)
            break
        if response["type"] != "stream_chunk" or not stream:
            raise ProtocolFailure("unexpected IPC message type")
        is_final, part = checked_stream_reply(response)
        if part and first_text_ms is None:
            first_text_ms = (time.perf_counter() - start) * 1000
        text_chars += len(part)
        if is_final:
            break
        frames += 1
    elapsed = (time.perf_counter() - start) * 1000
    if stream and not text_chars:
        raise ProtocolFailure("stream returned no sanitized output")
    return {"duration_ms": elapsed, "first_sanitized_text_ms": first_text_ms,
            "visible_chars": text_chars, "stream_data_frames": frames}
