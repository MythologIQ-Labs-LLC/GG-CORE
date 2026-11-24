"""Model-free negative controls for real Windows Ctrl+Break gate (#219)."""
import importlib.util
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

PATH = pathlib.Path(__file__).with_name("native_shutdown_console.py")
spec = importlib.util.spec_from_file_location("native_shutdown_console", PATH)
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class NativeShutdownTests(unittest.TestCase):
    def test_rejects_nonwindows_without_creating_output(self):
        with tempfile.TemporaryDirectory() as d:
            folder = pathlib.Path(d)
            out = folder / "evidence"
            with mock.patch.object(mod.platform, "system", return_value="Linux"):
                self.assertEqual(mod.main([
                    "--staged-root", str(folder), "--evidence-dir", str(out),
                    "--expected-head", "a" * 40, "--exe-sha256", "b" * 64
                ]), 1)
            self.assertFalse(out.exists())

    def test_offline_guest_must_prove_no_active_network_adapter(self):
        with mock.patch.object(mod.platform, "system", return_value="Windows"):
            with mock.patch.object(mod.subprocess, "run", return_value=(
                    subprocess.CompletedProcess([], 0, "1\\n", ""))):
                with self.assertRaisesRegex(RuntimeError, "NO-GO"):
                    mod.require_offline_windows()
            with mock.patch.object(mod.subprocess, "run", return_value=(
                    subprocess.CompletedProcess([], 1, "", "access denied"))):
                with self.assertRaisesRegex(RuntimeError, "NO-GO"):
                    mod.require_offline_windows()
            with mock.patch.object(mod.subprocess, "run", return_value=(
                    subprocess.CompletedProcess([], 0, "0\\n", ""))):
                mod.require_offline_windows()

    def test_bad_producer_sha_and_output_reuse_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            folder = pathlib.Path(d)
            self.assertRaises(ValueError, mod.require_inputs, folder, folder / "new",
                              "bad", "f" * 64)
            self.assertRaises(ValueError, mod.require_inputs, folder, folder,
                              "a" * 40, "f" * 64)
            (folder / "exists").mkdir()
            self.assertRaises(ValueError, mod.require_inputs, folder,
                              folder / "exists", "a" * 40, "f" * 64)

    def test_model_and_executable_identity_refuses_incorrect_bytes(self):
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d) / "stage"
            model = root / "models" / "qwen" / "model.gguf"
            model.parent.mkdir(parents=True)
            model.write_bytes(b"GGUF-not-approved")
            exe = root / "gg-core-cli.exe"
            exe.write_bytes(b"not-a-real-CLI")
            with self.assertRaisesRegex(ValueError, "Executable digest"):
                mod.require_inputs(root, pathlib.Path(d) / "evidence",
                                   "a" * 40, "f" * 64)
            with self.assertRaisesRegex(ValueError, "Unapproved model"):
                mod.require_inputs(root, pathlib.Path(d) / "evidence",
                                   "a" * 40, mod.sha256(exe))

    def test_digest_is_sensitive_to_content(self):
        with tempfile.TemporaryDirectory() as d:
            file = pathlib.Path(d) / "file"
            file.write_bytes(b"a")
            before = mod.sha256(file)
            file.write_bytes(b"b")
            self.assertNotEqual(before, mod.sha256(file))
            self.assertEqual(len(before), 64)

    def test_group_signal_refuses_invalid_pid_before_native_call(self):
        with self.assertRaisesRegex(ValueError, "invalid daemon"):
            mod.send_group_break(0)


if __name__ == "__main__":
    unittest.main()
