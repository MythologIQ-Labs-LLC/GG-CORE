#!/usr/bin/env python3
"""Offline fixture tests for benchmark framing/statistics, not a model benchmark."""
import json
import os
from pathlib import Path
import socket
import struct
import tempfile
import threading
import unittest
from argparse import Namespace

import bench_packaged_ipc as bench


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
                        peer.sendall(frame({"type": "handshake_ack", "session_id": "test", "protocol_version": "V1"}))
                elif not authed:
                    peer.sendall(frame({"type": "error", "code": 401, "message": "not authenticated"}))
                elif request["type"] == "warmup_request":
                    peer.sendall(frame({"type": "warmup_response", "model_id": request["model_id"], "success": True, "error": None, "elapsed_ms": 1}))
                elif request["type"] == "inference_request":
                    rid = request["request_id"]
                    if self.behavior == "wrong_id":
                        rid += 1
                    if request["parameters"]["stream"]:
                        messages = [
                            {"type": "stream_chunk", "request_id": rid, "is_final": False, "token": 123 if self.behavior == "raw_token" else 0, "text": "hello", "error": None},
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
                        peer.sendall(frame({"type": "inference_response", "request_id": rid, "output": "hello", "tokens_generated": 2, "finished": True, "error": None}))
                else:
                    peer.sendall(frame({"type": "error", "code": 400, "message": "bad request"}))

    def close(self):
        self.stopped.set()
        self.server.close()
        self.acceptor.join(timeout=2)
        for thread in self.threads:
            thread.join(timeout=2)
        self.temporary.cleanup()


@unittest.skipUnless(os.name == "posix", "local fixture requires Unix sockets; Windows named pipe awaits runner")
class BenchIpcTests(unittest.TestCase):
    def setUp(self):
        self.mock = MockDaemon()
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.prompt_file = self.root / "prompt.txt"
        self.prompt_file.write_text("A deliberately synthetic test prompt", encoding="utf8")
        self.model_file = self.root / "model.gguf"
        self.model_file.write_bytes(b"GGUF test fixture, not a model")
        self.binary = self.root / "daemon"
        self.binary.write_bytes(b"stub binary")
        self.prev_auth = os.environ.get("CORE_AUTH_TOKEN")
        os.environ["CORE_AUTH_TOKEN"] = "not-a-real-token-123456"

    def tearDown(self):
        if self.prev_auth is None:
            os.environ.pop("CORE_AUTH_TOKEN", None)
        else:
            os.environ["CORE_AUTH_TOKEN"] = self.prev_auth
        self.mock.close()
        self.tmp.cleanup()

    def args(self, **kwargs):
        data = dict(socket=self.mock.socket_path, model_id="qwen", model_file=self.model_file,
                    daemon_binary=self.binary, producer_sha="b" * 40, prompt_file=self.prompt_file,
                    auth_env="CORE_AUTH_TOKEN", stream=True, max_tokens=16,
                    timeout_ms=3000, concurrent=[1, 4], waves=2, output=self.root / "results.json")
        data.update(kwargs)
        return Namespace(**data)

    def test_sanitized_stream_quantiles_and_concurrency(self):
        result = bench.run_benchmark(self.args())
        self.assertEqual([x["stats"]["count"] for x in result["results"]], [2, 8])
        self.assertEqual(result["model_sha256"], bench.file_sha256(self.model_file))
        self.assertEqual(result["requested"]["security_and_sandbox_configuration"], "not independently established by benchmark")
        self.assertIsNotNone(result["results"][0]["stats"]["first_sanitized_text_ms"])
        self.assertNotIn(os.environ["CORE_AUTH_TOKEN"], json.dumps(result))
        self.assertNotIn("A deliberately synthetic test prompt", json.dumps(result))

    def test_nonstream_result_does_not_fabricate_ttft(self):
        result = bench.run_benchmark(self.args(stream=False, concurrent=[1], waves=1))
        self.assertIsNone(result["results"][0]["stats"]["first_sanitized_text_ms"])
        self.assertEqual(result["results"][0]["samples"][0]["visible_chars"], 5)

    def test_wrong_request_id_fails_closed(self):
        self.mock.close()
        self.mock = MockDaemon("wrong_id")
        with self.assertRaisesRegex(bench.ProtocolFailure, "request ID mismatch"):
            bench.run_benchmark(self.args())

    def test_bad_auth_fails_closed(self):
        self.mock.close()
        self.mock = MockDaemon("bad_auth")
        with self.assertRaisesRegex(bench.ProtocolFailure, "handshake"):
            bench.run_benchmark(self.args())

    def test_raw_token_ids_are_not_benchmarked_as_sanitized_text(self):
        self.mock.close()
        self.mock = MockDaemon("raw_token")
        with self.assertRaisesRegex(bench.ProtocolFailure, "raw token"):
            bench.run_benchmark(self.args())

    def test_lfs_pointer_is_rejected(self):
        self.model_file.write_bytes(b"version https://git-lfs.github.com/spec/v1")
        with self.assertRaisesRegex(ValueError, "Git LFS pointer"):
            bench.run_benchmark(self.args())

    def test_missing_auth_blocks_before_connection(self):
        os.environ["CORE_AUTH_TOKEN"] = "short"
        with self.assertRaisesRegex(ValueError, "16 bytes"):
            bench.run_benchmark(self.args())


class PureTests(unittest.TestCase):
    def test_linear_quantile(self):
        self.assertEqual(bench.quantile([40, 20, 10, 30], .5), 25)
        self.assertAlmostEqual(bench.quantile([40, 20, 10, 30], .95), 38.5)

    def test_reject_invalid_sha(self):
        with self.assertRaises(SystemExit):
            bench.parse_args(["--socket", "x", "--model-id", "q", "--model-file", "f",
                             "--daemon-binary", "x", "--producer-sha", "not-a-real-sha",
                             "--prompt-file", "f", "--output", "o"])

    def test_summary_requires_nonempty_samples(self):
        with self.assertRaises(ValueError):
            bench.summary([])


if __name__ == "__main__":
    unittest.main()