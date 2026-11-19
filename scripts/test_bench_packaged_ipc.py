#!/usr/bin/env python3
"""Offline fixtures for authenticated framed-IPC benchmark behavior."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from argparse import Namespace

import bench_packaged_ipc as bench
import bench_packaged_ipc_helpers as helpers
from bench_packaged_ipc_test_support import MockDaemon

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
        # No executable is launched during mock-IPC tests; still verify the
        # exact producer-identity contract through a controlled version result.
        self.version_probe = patch.object(
            bench, "verify_packaged_binary",
            return_value={
                "version": "0.99.0-dev", "commit": "b" * 40,
                "protocol_version": "V1",
            },
        )
        self.version_probe.start()
        self.prev_auth = os.environ.get("CORE_AUTH_TOKEN")
        os.environ["CORE_AUTH_TOKEN"] = "not-a-real-token-123456"

    def tearDown(self):
        self.version_probe.stop()
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
        self.assertEqual(result["schema_version"], 2)
        self.assertEqual(result["packaged_binary_version"]["commit"], "b" * 40)
        self.assertEqual(result["packaged_binary_version"]["protocol_version"], "V1")
        self.assertIn("handshake_ms", result)
        self.assertIn("warmup_request_ms", result)
        self.assertEqual(result["daemon_reported_warmup_ms"], 1)
        self.assertNotIn("warmup_auth_ms", result)
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

    def test_missing_handshake_session_is_rejected(self):
        self.mock.close()
        self.mock = MockDaemon("missing_session")
        with self.assertRaisesRegex(bench.ProtocolFailure, "handshake"):
            bench.run_benchmark(self.args())

    def test_empty_handshake_session_is_rejected(self):
        self.mock.close()
        self.mock = MockDaemon("empty_session")
        with self.assertRaisesRegex(bench.ProtocolFailure, "handshake"):
            bench.run_benchmark(self.args())

    def test_report_is_exclusive_and_remains_unchanged_on_retry(self):
        from unittest.mock import patch
        args = self.args(concurrent=[1], waves=1)
        with patch.object(bench, "parse_args", return_value=args):
            self.assertEqual(bench.main([]), 0)
            original = args.output.read_bytes()
            self.assertIn(b'"schema_version": 2', original)
            self.assertEqual(bench.main([]), 1)
            self.assertEqual(args.output.read_bytes(), original)
        self.assertEqual(list(args.output.parent.glob(".gg-core-bench-*.tmp")), [])

    def test_raw_token_ids_are_not_benchmarked_as_sanitized_text(self):
        self.mock.close()
        self.mock = MockDaemon("raw_token")
        with self.assertRaisesRegex(bench.ProtocolFailure, "raw token"):
            bench.run_benchmark(self.args())

    def test_warmup_model_identity_must_match(self):
        self.mock.close()
        self.mock = MockDaemon("wrong_warmup_model")
        with self.assertRaisesRegex(bench.ProtocolFailure, "warmup"):
            bench.run_benchmark(self.args())

    def test_warmup_elapsed_must_be_valid(self):
        self.mock.close()
        self.mock = MockDaemon("invalid_warmup_elapsed")
        with self.assertRaisesRegex(bench.ProtocolFailure, "warmup"):
            bench.run_benchmark(self.args())

    def test_nonstream_finished_flag_must_be_boolean(self):
        self.mock.close()
        self.mock = MockDaemon("invalid_finished_type")
        with self.assertRaisesRegex(bench.ProtocolFailure, "failed or incomplete"):
            bench.run_benchmark(self.args(stream=False))

    def test_nonstream_output_must_be_text(self):
        self.mock.close()
        self.mock = MockDaemon("invalid_output_type")
        with self.assertRaisesRegex(bench.ProtocolFailure, "no text"):
            bench.run_benchmark(self.args(stream=False))

    def test_boolean_raw_token_is_rejected(self):
        self.mock.close()
        self.mock = MockDaemon("boolean_token")
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
    def test_packaged_binary_identity_fails_closed_on_wrong_commit(self):
        expected = "b" * 40
        meta = {"name": "GG-CORE", "version": "1.0.0-dev",
                "commit": "c" * 40, "features": ["gguf"],
                "protocol_version": "V1"}
        with tempfile.TemporaryDirectory() as t:
            binary = Path(t) / "gg-core-cli"
            binary.write_bytes(b"not-an-executable")
            with patch.object(helpers.subprocess, "run",
                              return_value=subprocess.CompletedProcess(
                                  [], 0, json.dumps(meta).encode(), b"")):
                with self.assertRaisesRegex(bench.ProtocolFailure, "source commit mismatch"):
                    bench.verify_packaged_binary(binary, expected)

    def test_binary_metadata_failures_never_become_perf_evidence(self):
        expected = "b" * 40
        with tempfile.TemporaryDirectory() as t:
            binary = Path(t) / "gg-core-cli"
            binary.write_bytes(b"not-an-executable")
            good = {"name": "GG-CORE", "version": "1.0.0-dev",
                    "commit": expected, "features": ["gguf"],
                    "protocol_version": "V1"}
            cases = [
                (dict(good, protocol_version="V2"), "protocol mismatch"),
                (dict(good, features=["onnx"]), "lacks GGUF"),
                (dict(good, name="other"), "unexpected packaged"),
                (dict(good, version=""), "version absent"),
            ]
            for meta, message in cases:
                with self.subTest(message=message), patch.object(
                    helpers.subprocess, "run",
                    return_value=subprocess.CompletedProcess(
                        [], 0, json.dumps(meta).encode(), b""),
                ):
                    with self.assertRaisesRegex(bench.ProtocolFailure, message):
                        bench.verify_packaged_binary(binary, expected)
            for output in (b"not json", b"[]"):
                with self.subTest(output=output), patch.object(
                    helpers.subprocess, "run",
                    return_value=subprocess.CompletedProcess([], 0, output, b""),
                ):
                    with self.assertRaises(bench.ProtocolFailure):
                        bench.verify_packaged_binary(binary, expected)
            with patch.object(
                helpers.subprocess, "run",
                return_value=subprocess.CompletedProcess([], 2, b"", b"sensitive"),
            ):
                with self.assertRaisesRegex(bench.ProtocolFailure, "version command refused"):
                    bench.verify_packaged_binary(binary, expected)

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

    def test_tail_percentiles_require_minimum_observations(self):
        def samples(n):
            return [{"duration_ms": float(i + 1), "first_sanitized_text_ms": float(i + 1)}
                    for i in range(n)]
        small = bench.summary(samples(10))
        self.assertEqual(small["duration_ms"]["p50"], 5.5)
        self.assertIsNone(small["duration_ms"]["p95"])
        self.assertIsNone(small["duration_ms"]["p99"])
        medium = bench.summary(samples(20))
        self.assertIsNotNone(medium["duration_ms"]["p95"])
        self.assertIsNone(medium["duration_ms"]["p99"])
        large = bench.summary(samples(100))
        self.assertIsNotNone(large["duration_ms"]["p99"])


if __name__ == "__main__":
    unittest.main()