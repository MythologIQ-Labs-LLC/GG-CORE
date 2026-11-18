"""Model-free, cross-platform regressions for the Windows evidence collector."""

import importlib.util
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

PATH = pathlib.Path(__file__).with_name("native_windows_audit_gate.py")
spec = importlib.util.spec_from_file_location("native_windows_audit_gate", PATH)
assert spec and spec.loader
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class NativeAcceptanceContractTests(unittest.TestCase):
    def test_offline_locked_matrix_never_skips_external_audit_or_full_features(self):
        steps = dict(native.command_matrix())
        self.assertEqual(set(steps), {
            "metadata", "fmt", "clippy_default", "clippy_full", "audit_wire",
            "test_default", "test_advanced", "test_ffi", "test_python",
            "test_onnx", "test_gguf", "test_full", "cargo_deny", "release_build",
        })
        for label, command in steps.items():
            self.assertEqual(command[0], "cargo")
            if label != "fmt" and label != "cargo_deny":
                self.assertIn("--locked", command, label)
                self.assertIn("--offline", command, label)
        self.assertIn("--no-fail-fast", steps["test_full"])
        self.assertIn("audit_timestamp_compat_test", steps["audit_wire"])

    def test_failed_native_step_is_a_nonzero_receipt_with_log(self):
        with tempfile.TemporaryDirectory() as root:
            folder = pathlib.Path(root)
            report = {"steps": []}
            with self.assertRaisesRegex(RuntimeError, "exit=17"):
                native.run_step("negative", [sys.executable, "-c",
                                             "import sys; print('test result: FAILED.'); sys.exit(17)"],
                                folder, {}, folder, report)
            row = report["steps"][0]
            self.assertEqual(row["exit_code"], 17)
            self.assertEqual(row["test_summaries"], ["test result: FAILED."])
            self.assertTrue((folder / "negative.log").exists())

    def test_real_fixture_refuses_missing_or_lfs(self):
        with tempfile.TemporaryDirectory() as root:
            repo = pathlib.Path(root)
            path = repo / native.MODEL_RELATIVE
            path.parent.mkdir(parents=True)
            path.write_text("version https://git-lfs.github.com/spec/v1\n")
            with mock.patch.object(native, "checked_stdout", side_effect=["a" * 40, ""]):
                with self.assertRaisesRegex(RuntimeError, "not GGUF"):
                    native.validate_checkout(repo, "a" * 40, "b" * 64)

    def test_git_head_and_checksum_are_mandatory(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(ValueError):
                native.validate_checkout(pathlib.Path(root), "bad", "b" * 64)
            with mock.patch.object(native, "checked_stdout", return_value="b" * 40):
                with self.assertRaisesRegex(RuntimeError, "differs"):
                    native.validate_checkout(pathlib.Path(root), "a" * 40, "b" * 64)

    def test_untracked_source_fails_exact_head_even_when_commit_matches(self):
        with tempfile.TemporaryDirectory() as root:
            with mock.patch.object(native, "checked_stdout", side_effect=["a" * 40, "?? core-runtime/src/sidecar.rs"]):
                with self.assertRaisesRegex(RuntimeError, "Uncommitted or untracked source"):
                    native.validate_checkout(pathlib.Path(root), "a" * 40, "b" * 64)

    def test_linux_cannot_be_certified_as_native_windows(self):
        with mock.patch.object(native.platform, "system", return_value="Linux"):
            with self.assertRaisesRegex(RuntimeError, "requires Windows"):
                native.require_offline_windows()

    def test_cli_provenance_must_match_source_and_features(self):
        with tempfile.TemporaryDirectory() as root:
            core = pathlib.Path(root)
            dep = core / "target/debug/deps"
            dep.mkdir(parents=True)
            (dep / "gg_core-deadbeef.exe").write_bytes(b"test")
            release = core / "target/release/gg-core-cli.exe"
            release.parent.mkdir(parents=True)
            release.write_bytes(b"cli")
            with mock.patch.object(native, "checked_stdout", return_value=json.dumps(
                {"commit": "b" * 40, "features": ["onnx", "gguf", "ffi"]}
            )):
                with self.assertRaisesRegex(RuntimeError, "commit does not match"):
                    native.collect_artifacts(core, {"source_sha": "a" * 40})
            with mock.patch.object(native, "checked_stdout", return_value=json.dumps(
                {"commit": "a" * 40, "features": ["gguf"]}
            )):
                with self.assertRaisesRegex(RuntimeError, "feature provenance differs"):
                    native.collect_artifacts(core, {"source_sha": "a" * 40})

    def test_main_writes_hold_receipt_on_non_windows(self):
        with tempfile.TemporaryDirectory() as root:
            destination = pathlib.Path(root) / "evidence"
            with mock.patch.object(native.platform, "system", return_value="Linux"):
                result = native.main([
                    "--expected-head", "a" * 40, "--model-sha256", "b" * 64,
                    "--evidence-dir", str(destination),
                ])
            self.assertEqual(result, 1)
            receipt = json.loads((destination / "receipt.json").read_text())
            self.assertEqual(receipt["status"], "HOLD")
            self.assertIn("requires Windows", receipt["error"])
            self.assertEqual(receipt["steps"], [])

    def test_no_windows_test_binaries_never_passes(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(RuntimeError, "No compiled Windows test binaries"):
                native.collect_artifacts(pathlib.Path(root), {})


if __name__ == "__main__":
    unittest.main()
