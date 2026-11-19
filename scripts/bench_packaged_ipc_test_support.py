"""Offline mock daemon for model-free packaged IPC probe regression tests."""
import json
from pathlib import Path
import socket
import struct
import tempfile
import threading

def frame(message):
    b = json.dumps(message).encode("utf-8")
    return struct.pack("<I", len(b)) + b


def read_exact(sock, n):
    out = bytearray()
    while len(out) < n:
        data = sock.recv(n - len(out))
        if not data:
            raise EOFError("peer closed")
        out.extend(data)
    return bytes(out)


class MockDaemon:
    def __init__(self, behavior="ok"):
        self.behavior = behavior
        self.temporary = tempfile.TemporaryDirectory()
        self.socket_path = str(Path(self.temporary.name) / "gg.sock")
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(self.socket_path)
        self.server.listen(32)
        self.server.settimeout(0.1)
        self.stopped = threading.Event()
        self.threads = []
        self.acceptor = threading.Thread(target=self.accept, daemon=True)
        self.acceptor.start()

    def accept(self):
        while not self.stopped.is_set():
            try:
                peer, _ = self.server.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            thread = threading.Thread(target=self.handle, args=(peer,), daemon=True)
            thread.start()
            self.threads.append(thread)

    def handle(self, peer):
        with peer:
            authed = False
            while not self.stopped.is_set():
                try:
                    size = struct.unpack("<I", read_exact(peer, 4))[0]
                    request = json.loads(read_exact(peer, size))
                except (EOFError, OSError):
                    return
                if request["type"] == "handshake":
                    if self.behavior == "bad_auth":
                        peer.sendall(frame({"type": "error", "code": 401, "message": "denied"}))
                    else:
                        authed = True
                        ack = {"type": "handshake_ack", "session_id": "test", "protocol_version": "V1"}
                        if self.behavior == "missing_session":
                            ack.pop("session_id")
                        elif self.behavior == "empty_session":
                            ack["session_id"] = ""
                        peer.sendall(frame(ack))
                elif not authed:
                    peer.sendall(frame({"type": "error", "code": 401, "message": "not authenticated"}))
                elif request["type"] == "warmup_request":
                    model_id = "unexpected-model" if self.behavior == "wrong_warmup_model" else request["model_id"]
                    elapsed = -1 if self.behavior == "invalid_warmup_elapsed" else 1
                    peer.sendall(frame({"type": "warmup_response", "model_id": model_id, "success": True, "error": None, "elapsed_ms": elapsed}))
                elif request["type"] == "inference_request":
                    rid = request["request_id"]
                    if self.behavior == "wrong_id":
                        rid += 1
                    if request["parameters"]["stream"]:
                        messages = [
                            {"type": "stream_chunk", "request_id": rid, "is_final": False, "token": (123 if self.behavior == "raw_token" else True if self.behavior == "boolean_token" else 0), "text": "hello", "error": None},
                            {"type": "stream_chunk", "request_id": rid, "is_final": True, "token": 0, "text": None, "error": None},
                        ]
                        # Send two messages in one write, in small chunks to stress frame parsing.
                        wire = b"".join(frame(message) for message in messages)
                        try:
                            for position in range(0, len(wire), 7):
                                peer.sendall(wire[position:position + 7])
                        except (BrokenPipeError, ConnectionResetError):
                            return
                    else:
                        output = 123 if self.behavior == "invalid_output_type" else "hello"
                        finished = "true" if self.behavior == "invalid_finished_type" else True
                        peer.sendall(frame({"type": "inference_response", "request_id": rid, "output": output, "tokens_generated": 2, "finished": finished, "error": None}))
                else:
                    peer.sendall(frame({"type": "error", "code": 400, "message": "bad request"}))

    def close(self):
        self.stopped.set()
        self.server.close()
        self.acceptor.join(timeout=2)
        for thread in self.threads:
            thread.join(timeout=2)
        self.temporary.cleanup()
