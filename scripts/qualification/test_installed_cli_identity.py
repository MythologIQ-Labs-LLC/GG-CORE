"""Model-free negative controls for installed_cli_identity (#114)."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import conformance_receipts as receipts
import installed_cli_identity as identity

SHA = "a" * 40
CASE = "cli_version_default_installed"
MANIFEST = json.loads(Path(__file__).with_name("conformance_inventory.json").read_text())


class InstalledCliIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.name = "gg-core-cli"
        self.binary = self.root / self.name
        self.binary.write_bytes(b"mock staged executable, never executed")
        self.digest = hashlib.sha256(self.binary.read_bytes()).hexdigest()
        self.platform = identity.TARGET_PLATFORMS[sys.platform]

    def version(self, **changes):
        doc = {"name": "GG-CORE", "commit": SHA, "version": "0.1.0",
               "protocol_version": "V1", "target": self.platform + "-x86_64",
               "features": []}
        doc.update(changes)
        return subprocess.CompletedProcess([], 0, json.dumps(doc).encode(), b"")

    def check(self, result=None, **changes):
        return identity.probe(MANIFEST, CASE, self.root, self.name, self.digest,
                              SHA, runner=lambda *a, **kw: result or self.version(**changes))

    def test_real_file_hash_and_version_source_match_produce_only_recorded_pass(self):
        item = self.check()
        self.assertFalse(item["evidence_attested"])
        self.assertEqual(item["binary_sha256"], self.digest)
        output = receipts.report(MANIFEST, [item], SHA)
        self.assertEqual(output["status"], "HOLD")
        self.assertFalse(output["release_authorized"])
        self.assertEqual(output["counts"]["RECORDED_PASS"], 1)

    def test_mismatch_sha_and_symlink_fail_before_execution(self):
        self.binary.write_bytes(b"changed")
        with self.assertRaisesRegex(identity.ProbeError, "binary-sha256-mismatch"):
            self.check()
        self.binary.unlink()
        self.binary.symlink_to(self.root / "elsewhere")
        with self.assertRaisesRegex(identity.ProbeError, "missing-or-symlinked"):
            self.check()

    def test_executable_mutation_during_probe_refuses_receipt(self):
        def replace(*args, **kwargs):
            self.binary.write_bytes(b"binary changed during execution")
            return self.version()
        with self.assertRaisesRegex(identity.ProbeError, "binary-changed-during-probe"):
            identity.probe(MANIFEST, CASE, self.root, self.name,
                           self.digest, SHA, runner=replace)

    def test_source_features_protocol_and_target_refuse_false_identity(self):
        for changes, code in [
            ({"commit": "b" * 40}, "source-commit-mismatch"),
            ({"features": ["gguf"]}, "compiled-feature-mismatch"),
            ({"features": ["onnx", "onnx"]}, "compiled-feature-mismatch"),
            ({"protocol_version": "V2"}, "unsupported-protocol-version"),
            ({"target": "not-a-target"}, "invalid-target"),
            ({"target": "windows-x86_64" if self.platform != "windows"
             else "linux-x86_64"}, "target-platform-mismatch"),
            ({"name": "impostor"}, "unexpected-cli-identity"),
        ]:
            with self.subTest(changes=changes):
                with self.assertRaisesRegex(identity.ProbeError, code):
                    self.check(**changes)

    def test_malformed_duplicate_json_nonfinite_and_large_output_fail(self):
        for payload, code in [
            (b'{"name":"GG-CORE","name":"GG-CORE"}', "cli-version-invalid-json"),
            (b'{"value":NaN}', "cli-version-invalid-json"),
            (b"{" + b"x" * 9000 + b"}", "cli-version-output-too-large"),
        ]:
            with self.subTest(code=code):
                result = subprocess.CompletedProcess([], 0, payload, b"")
                with self.assertRaisesRegex(identity.ProbeError, code):
                    self.check(result=result)

    def test_command_refusal_timeout_and_invalid_case_fail_closed(self):
        with self.assertRaisesRegex(identity.ProbeError, "cli-version-exit-nonzero"):
            self.check(result=subprocess.CompletedProcess([], 1, b"", b"secret"))
        with self.assertRaisesRegex(identity.ProbeError, "cli-version-execution-failed"):
            identity.probe(MANIFEST, CASE, self.root, self.name, self.digest, SHA,
                           runner=lambda *a, **kw: (_ for _ in ()).throw(
                               subprocess.TimeoutExpired(["gg-core-cli"], 10)))
        with self.assertRaisesRegex(identity.ProbeError, "unknown-installed-cli-case"):
            identity.probe(MANIFEST, "rust_external_v1", self.root, self.name,
                           self.digest, SHA, runner=lambda *a, **kw: self.version())

    def test_full_feature_case_rejects_default_compilation(self):
        with self.assertRaisesRegex(identity.ProbeError, "compiled-feature-mismatch"):
            identity.probe(MANIFEST, "cli_version_onnx_gguf_ffi_installed",
                           self.root, self.name, self.digest, SHA,
                           runner=lambda *a, **kw: self.version())
        full = identity.probe(
            MANIFEST, "cli_version_onnx_gguf_ffi_installed", self.root,
            self.name, self.digest, SHA,
            runner=lambda *a, **kw: self.version(features=["ffi", "gguf", "onnx"]))
        self.assertEqual(full["features"], ["onnx", "gguf", "ffi"])

    def test_exclusive_receipt_mode_and_no_overwrite(self):
        path = self.root / "receipt.json"
        identity.exclusive_receipt(path, [{"status": "PASS"}])
        before = path.read_bytes()
        with self.assertRaises(FileExistsError):
            identity.exclusive_receipt(path, [{"status": "FAIL"}])
        self.assertEqual(path.read_bytes(), before)
        if sys.platform != "win32":
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)


if __name__ == "__main__":
    unittest.main()
