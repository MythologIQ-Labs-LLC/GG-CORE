"""Cross-platform model-free negative tests for binary identity preflight (#114)."""
import hashlib
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

from check_binary_identity import check

COMMIT = "a" * 40


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.binary = pathlib.Path(self.temp.name) / "gg-core-cli.bin"
        self.binary.write_bytes(b"GG-CORE identity fixture; not executable")
        self.digest = hashlib.sha256(self.binary.read_bytes()).hexdigest()
        self.version = {"name": "GG-CORE", "version": "1.0.0", "commit": COMMIT,
                        "features": ["gguf", "onnx"], "protocol_version": "V1",
                        "target": "linux-x86_64"}

    def run_check(self, commit=COMMIT, digest=None, required=None):
        response = subprocess.CompletedProcess([], 0, stdout=json.dumps(self.version))
        with mock.patch("check_binary_identity.subprocess.run", return_value=response) as invoke:
            receipt = check(self.binary, commit, digest or self.digest,
                            required if required is not None else {"gguf"})
        invoke.assert_called_once()
        self.assertEqual(invoke.call_args.args[0][1:], ["version", "--json"])
        return receipt

    def test_positive_receipt_is_narrow(self):
        receipt = self.run_check()
        self.assertEqual(receipt["result"], "PASS")
        self.assertFalse(receipt["native_inference_qualified"])
        self.assertFalse(receipt["security_qualified"])

    def test_modified_binary_refused(self):
        with self.assertRaisesRegex(ValueError, "binary SHA-256 mismatch"):
            self.run_check(digest="0" * 64)

    def test_wrong_source_refused(self):
        with self.assertRaisesRegex(ValueError, "compiled source SHA mismatch"):
            self.run_check(commit="b" * 40)

    def test_unavailable_feature_refused(self):
        with self.assertRaisesRegex(ValueError, "missing compiled features"):
            self.run_check(required={"cuda"})

    def test_duplicate_capabilities_refused(self):
        self.version["features"] = ["gguf", "gguf"]
        with self.assertRaisesRegex(ValueError, "malformed compiled feature list"):
            self.run_check()

    def test_symlink_refused(self):
        link = self.binary.with_suffix(".link")
        try:
            link.symlink_to(self.binary)
        except (OSError, NotImplementedError):
            self.skipTest("symlink privileges unavailable on this host")
        with self.assertRaisesRegex(ValueError, "not a symlink"):
            check(link, COMMIT, self.digest, set())

    def test_short_source_sha_refused(self):
        with self.assertRaisesRegex(ValueError, "exact 40-digit"):
            self.run_check(commit="abc")

    def test_non_string_compiled_commit_refused(self):
        self.version["commit"] = None
        with self.assertRaisesRegex(ValueError, "compiled source SHA mismatch"):
            self.run_check()

    def test_unexpected_protocol_refused(self):
        self.version["protocol_version"] = "V2"
        with self.assertRaisesRegex(ValueError, "unexpected IPC protocol version"):
            self.run_check()

    def test_changed_binary_during_invocation_refused(self):
        response = subprocess.CompletedProcess([], 0, stdout=json.dumps(self.version))
        def mutate(*args, **kwargs):
            self.binary.write_bytes(b"modified during version invocation")
            return response
        with mock.patch("check_binary_identity.subprocess.run", side_effect=mutate):
            with self.assertRaisesRegex(ValueError, "binary changed during identity check"):
                check(self.binary, COMMIT, self.digest, set())

    def test_nonzero_version_exit_refused(self):
        response = subprocess.CompletedProcess([], 2, stdout="")
        with mock.patch("check_binary_identity.subprocess.run", return_value=response):
            with self.assertRaisesRegex(ValueError, "version command failed"):
                check(self.binary, COMMIT, self.digest, set())

    def test_timeout_refused(self):
        with mock.patch("check_binary_identity.subprocess.run",
                        side_effect=subprocess.TimeoutExpired("gg-core-cli", 15)):
            with self.assertRaisesRegex(ValueError, "version invocation failed"):
                check(self.binary, COMMIT, self.digest, set())

    def test_unknown_required_feature_refused(self):
        with self.assertRaisesRegex(ValueError, "unknown required feature"):
            self.run_check(required={"not_a_backend"})


if __name__ == "__main__":
    unittest.main()
