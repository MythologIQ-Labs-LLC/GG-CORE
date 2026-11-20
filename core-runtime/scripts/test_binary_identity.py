"""Model-free negative tests for binary identity preflight (#114)."""
import hashlib
import pathlib
import tempfile
import unittest

from check_binary_identity import check

COMMIT = "a" * 40


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.binary = pathlib.Path(self.temp.name) / "gg-core-cli"
        self.write_fixture(COMMIT, '["gguf","onnx"]')

    def write_fixture(self, commit, features):
        self.binary.write_text(
            '#!/usr/bin/env python3\n'
            'import json\n'
            "print(json.dumps({'name':'GG-CORE','version':'1.0.0','commit':"
            + repr(commit) + ",'features':" + features
            + ",'protocol_version':'V1','target':'linux-x86_64'}))\n"
        )
        self.binary.chmod(0o700)
        self.digest = hashlib.sha256(self.binary.read_bytes()).hexdigest()

    def test_positive_receipt_is_narrow(self):
        receipt = check(self.binary, COMMIT, self.digest, {"gguf"})
        self.assertEqual(receipt["result"], "PASS")
        self.assertFalse(receipt["native_inference_qualified"])
        self.assertFalse(receipt["security_qualified"])

    def test_modified_binary_refused(self):
        with self.assertRaisesRegex(ValueError, "binary SHA-256 mismatch"):
            check(self.binary, COMMIT, "0" * 64, set())

    def test_wrong_source_refused(self):
        with self.assertRaisesRegex(ValueError, "compiled source SHA mismatch"):
            check(self.binary, "b" * 40, self.digest, set())

    def test_unavailable_feature_refused(self):
        with self.assertRaisesRegex(ValueError, "missing compiled features"):
            check(self.binary, COMMIT, self.digest, {"cuda"})

    def test_duplicate_capabilities_refused(self):
        self.write_fixture(COMMIT, '["gguf","gguf"]')
        with self.assertRaisesRegex(ValueError, "malformed compiled feature list"):
            check(self.binary, COMMIT, self.digest, set())

    def test_symlink_refused(self):
        link = self.binary.with_suffix(".link")
        link.symlink_to(self.binary)
        with self.assertRaisesRegex(ValueError, "not a symlink"):
            check(link, COMMIT, self.digest, set())

    def test_short_source_sha_refused(self):
        with self.assertRaisesRegex(ValueError, "exact 40-digit"):
            check(self.binary, "abc", self.digest, set())


if __name__ == "__main__":
    unittest.main()
