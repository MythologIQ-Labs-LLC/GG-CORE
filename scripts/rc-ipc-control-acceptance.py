#!/usr/bin/env python3
"""Acceptance against a running, packaged, real-GGUF GG-CORE daemon.

Called by rc-acceptance.sh with CORE_AUTH_TOKEN, GG_CORE_SOCKET_PATH and
qwen preloaded. Python stdlib only. No model download or host-state changes.
"""
import ctypes
import json
import os
import socket
import struct
import sys
import time

MODEL_ID = "qwen"
# Relative to the daemon's base path (its working directory), inside models/.
MODEL_REL = os.environ.get("GG_CORE_MODEL_REL", "models/qwen2.5-0.5b-instruct-q4_k_m.gguf")
TIMEOUT_S = 90
MAX_FRAME = 16 * 1024 * 1024


class Client:
    def __init__(self):
        addr = os.environ["GG_CORE_SOCKET_PATH"]
        self.deadline = time.monotonic() + TIMEOUT_S
        self.pipe = None
        self.sock = None
        if os.name == "nt":
            # The native Windows named pipe is a byte stream; use its ordinary
            # Win32 file handle, with PeekNamedPipe for bounded reads.
            self.pipe = open(addr, "r+b", buffering=0)
            import msvcrt
            from ctypes import wintypes
            handle = msvcrt.get_osfhandle(self.pipe.fileno())
            self.handle = handle
            peek = ctypes.WinDLL("kernel32", use_last_error=True).PeekNamedPipe
            peek.argtypes = (
                wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p
            )
            peek.restype = wintypes.BOOL
            self.peek = peek
        else:
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.settimeout(TIMEOUT_S)
            self.sock.connect(addr)

    def close(self):
        if self.pipe is not None:
            self.pipe.close()
        if self.sock is not None:
            self.sock.close()

    def send(self, msg):
        data = json.dumps(msg, separators=(",", ":")).encode("utf-8")
        if len(data) > MAX_FRAME:
            raise ValueError("oversized IPC request")
        frame = struct.pack("<I", len(data)) + data
        if self.pipe:
            self.pipe.write(frame)
        else:
            self.sock.sendall(frame)

    def read_exact(self, length):
        data = bytearray()
        while len(data) < length:
            remaining_time = self.deadline - time.monotonic()
            if remaining_time <= 0:
                raise TimeoutError("bounded IPC response timed out")
            if self.pipe:
                avail = ctypes.c_ulong()
                if not self.peek(self.handle, None, 0, None, ctypes.byref(avail), None):
                    raise ctypes.WinError(ctypes.get_last_error())
                if not avail.value:
                    time.sleep(0.02)
                    continue
                chunk = self.pipe.read(min(length - len(data), avail.value))
            else:
                self.sock.settimeout(remaining_time)
                chunk = self.sock.recv(length - len(data))
            if not chunk:
                raise EOFError("daemon closed the IPC connection")
            data.extend(chunk)
        return bytes(data)

    def recv(self):
        length = struct.unpack("<I", self.read_exact(4))[0]
        if length == 0 or length > MAX_FRAME:
            raise AssertionError("invalid IPC response length: " + str(length))
        return json.loads(self.read_exact(length).decode("utf-8"))

    def auth(self):
        self.send({
            "type": "handshake", "token": os.environ["CORE_AUTH_TOKEN"],
            "protocol_version": "V1"
        })
        response = self.recv()
        assert response.get("type") == "handshake_ack", response


def stream_request(request_id, timeout_ms):
    return {
        "type": "inference_request", "request_id": request_id,
        "model_id": MODEL_ID,
        "prompt": "Write a detailed numbered list of 100 practical ways to test a local application. Explain each item in two complete sentences.",
        "parameters": {
            "stream": True, "timeout_ms": timeout_ms, "max_tokens": 2048,
            "temperature": 0.0, "top_p": 1.0, "top_k": 1
        }
    }


def check_warmup_auth():
    c = Client()
    try:
        c.send({"type": "warmup_request", "model_id": MODEL_ID, "tokens": 1})
        frame = c.recv()
        assert frame.get("type") == "error" and frame.get("code") == 401, frame
    finally:
        c.close()
    print("PASS: unauthenticated compute-producing warmup refused", flush=True)


def check_auth_warmup():
    c = Client()
    try:
        c.auth()
        c.send({"type": "warmup_request", "model_id": MODEL_ID, "tokens": 1})
        frame = c.recv()
        assert frame.get("type") == "warmup_response" and frame.get("success") is True, frame
    finally:
        c.close()
    print("PASS: authenticated warmup executes against loaded model", flush=True)


def cancel_active_stream(c, rid):
    """Start a stream, wait for real generated text, cancel it on the same
    connection, and require ACK + failed terminal with no data after the ACK."""
    c.send(stream_request(rid, 90000))
    # Do not accept cancelling an idle queue as proof of cancelling a
    # generating model. Require actual sanitized generated text first.
    for _ in range(1000):
        frame = c.recv()
        assert frame.get("type") == "stream_chunk" and frame.get("request_id") == rid, frame
        assert not frame.get("error") and not frame.get("is_final"), frame
        if frame.get("text"):
            break
    else:
        raise AssertionError("model generated no text before cancellation")
    c.send({"type": "cancel_request", "request_id": rid})
    acknowledged = terminal = False
    for _ in range(100):
        frame = c.recv()
        if frame.get("type") == "cancel_response":
            assert frame.get("request_id") == rid and frame.get("cancelled") is True, frame
            acknowledged = True
        elif frame.get("type") == "stream_chunk":
            assert frame.get("request_id") == rid, frame
            if frame.get("is_final"):
                assert frame.get("error") and "cancel" in frame["error"].lower(), frame
                terminal = True
            elif acknowledged:
                raise AssertionError("data emitted after cancel acknowledgement")
        else:
            raise AssertionError("unexpected IPC frame: " + repr(frame))
        if acknowledged and terminal:
            break
    assert acknowledged and terminal, "missing cancel acknowledgement or error terminal"


def check_active_cancel():
    c = Client()
    try:
        c.auth()
        cancel_active_stream(c, 4242)
    finally:
        c.close()
    print("PASS: active real-model generation cancelled with failed terminal", flush=True)


def check_switch_after_cancel():
    """Cancel, then switch models: the native generation loop may still be
    winding down to its next checkpoint, so its memory budget can still be
    held when the unload answers. The consumer waits for the release of the
    unloaded instance before loading (a load may otherwise be refused while
    the old instance is still charged)."""
    c = Client()
    try:
        c.auth()
        cancel_active_stream(c, 4244)
        c.send({"type": "model_unload_request", "model_id": MODEL_ID})
        frame = c.recv()
        assert frame.get("type") == "model_unload_response" and frame.get("success") is True, frame
        c.send({"type": "model_release_wait_request", "model_id": MODEL_ID, "wait_ms": 60000})
        frame = c.recv()
        assert frame.get("type") == "model_release_wait_response" and frame.get("released") is True, frame
        c.send({"type": "model_load_request", "path": MODEL_REL, "model_id": MODEL_ID})
        frame = c.recv()
        assert frame.get("type") == "model_load_response" and frame.get("success") is True, frame
        c.send({
            "type": "inference_request", "request_id": 4245, "model_id": MODEL_ID,
            "prompt": "Reply with the single word OK.",
            "parameters": {"stream": False, "timeout_ms": 60000, "max_tokens": 4}
        })
        frame = c.recv()
        assert frame.get("type") == "inference_response" and not frame.get("error"), frame
    finally:
        c.close()
    print("PASS: model unload, verified release and reload after cancellation", flush=True)


def check_deadline():
    c = Client()
    try:
        c.auth()
        rid = 4243
        start = time.monotonic()
        c.send(stream_request(rid, 350))
        for _ in range(1000):
            frame = c.recv()
            assert frame.get("type") == "stream_chunk" and frame.get("request_id") == rid, frame
            if frame.get("is_final"):
                assert frame.get("error"), "deadline produced false success"
                assert ("deadline" in frame["error"].lower() or
                        "cancel" in frame["error"].lower()), frame
                break
        else:
            raise AssertionError("no terminal response")
        assert time.monotonic() - start < 15, "deadline response exceeded bound"
    finally:
        c.close()
    print("PASS: streaming inference deadline returns failed terminal", flush=True)


FOX = "The brown fox walks beside a clear river under a quiet blue sky. "
# About 2,500 Qwen tokens: seconds of prompt prefill before the first token.
LONG_PROMPT = "Reply with the word READY after reading this note: " + FOX * 179


def nonstream_request(request_id, prompt, max_tokens, timeout_ms):
    return {
        "type": "inference_request", "request_id": request_id,
        "model_id": MODEL_ID, "prompt": prompt,
        "parameters": {
            "stream": False, "timeout_ms": timeout_ms, "max_tokens": max_tokens,
            "temperature": 0.0, "top_p": 1.0, "top_k": 1
        }
    }


def assert_stopped(frame, rid, code):
    """A stopped request is an explicit failure, never a success-shaped reply."""
    assert frame.get("type") == "inference_response" and frame.get("request_id") == rid, frame
    assert frame.get("error") and frame.get("error_code") == code, frame
    assert not frame.get("output") and not frame.get("tokens_generated"), frame


def short_request_ok(rid):
    """The worker, context and queue slot were released: a new request runs."""
    c = Client()
    try:
        c.auth()
        start = time.monotonic()
        c.send(nonstream_request(rid, "Reply with the single word OK.", 4, 120000))
        frame = c.recv()
        assert frame.get("type") == "inference_response" and not frame.get("error"), frame
        assert frame.get("output"), frame
        return time.monotonic() - start
    finally:
        c.close()


def check_nonstream_prefill_deadline():
    c = Client()
    try:
        c.auth()
        start = time.monotonic()
        c.send(nonstream_request(4250, LONG_PROMPT, 256, 350))
        assert_stopped(c.recv(), 4250, "deadline_exceeded")
        elapsed = time.monotonic() - start
        assert elapsed < 5, "non-streaming deadline ignored: %.2fs" % elapsed
    finally:
        c.close()
    after = short_request_ok(4251)
    print("PASS: non-streaming timeout_ms=350 during long prefill -> deadline_exceeded "
          "in %.0f ms; next request %.0f ms" % (elapsed * 1000, after * 1000), flush=True)


def check_nonstream_prefill_cancel():
    c = Client()
    try:
        c.auth()
        c.send(nonstream_request(4252, LONG_PROMPT, 256, 90000))
        time.sleep(0.3)
        start = time.monotonic()
        c.send({"type": "cancel_request", "request_id": 4252})
        acknowledged = stopped = False
        for _ in range(2):
            frame = c.recv()
            if frame.get("type") == "cancel_response":
                assert frame.get("request_id") == 4252 and frame.get("cancelled") is True, frame
                acknowledged = True
            else:
                assert_stopped(frame, 4252, "cancelled")
                stopped = True
        assert acknowledged and stopped, "missing cancel ACK or explicit failure"
        elapsed = time.monotonic() - start
    finally:
        c.close()
    after = short_request_ok(4253)
    print("PASS: non-streaming cancel_request during long prefill -> cancelled in %.0f ms; "
          "next request %.0f ms" % (elapsed * 1000, after * 1000), flush=True)


def check_nonstream_disconnect():
    c = Client()
    try:
        c.auth()
        c.send(nonstream_request(4254, LONG_PROMPT, 256, 90000))
        time.sleep(0.3)
    finally:
        c.close()
    after = short_request_ok(4255)
    print("PASS: disconnect during long non-streaming prefill releases the worker; "
          "next request %.0f ms" % (after * 1000), flush=True)


def check_concurrent_isolation():
    """Cancelling one connection's request must not affect another's."""
    other = "Repeat the first sentence of this note: " + FOX * 40
    ref = Client()
    try:
        ref.auth()
        ref.send(nonstream_request(4256, other, 12, 120000))
        reference = ref.recv()
        assert not reference.get("error"), reference
    finally:
        ref.close()
    a, b = Client(), Client()
    try:
        a.auth()
        b.auth()
        a.send(nonstream_request(4257, LONG_PROMPT, 256, 90000))
        time.sleep(0.1)
        b.send(nonstream_request(4258, other, 12, 120000))
        time.sleep(0.3)
        a.send({"type": "cancel_request", "request_id": 4257})
        frames = [a.recv(), a.recv()]
        assert any(f.get("type") == "cancel_response" and f.get("cancelled") for f in frames), frames
        assert_stopped(next(f for f in frames if f.get("type") == "inference_response"),
                       4257, "cancelled")
        survivor = b.recv()
        assert survivor.get("type") == "inference_response" and not survivor.get("error"), survivor
        assert survivor.get("output") == reference.get("output"), (survivor, reference)
    finally:
        a.close()
        b.close()
    print("PASS: cancelling one request leaves a concurrent request's output unchanged",
          flush=True)


def main():
    for key in ("CORE_AUTH_TOKEN", "GG_CORE_SOCKET_PATH"):
        if not os.environ.get(key):
            raise RuntimeError("missing required environment variable " + key)
    check_warmup_auth()
    check_auth_warmup()
    check_active_cancel()
    check_deadline()
    check_nonstream_prefill_deadline()
    check_nonstream_prefill_cancel()
    check_nonstream_disconnect()
    check_concurrent_isolation()
    check_switch_after_cancel()


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print("FAIL: packaged real-model IPC control acceptance: " + repr(exc), file=sys.stderr)
        raise SystemExit(1)
