"""Model-free negative controls for the CUDA hidden-device evidence parser."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

MODULE = Path(__file__).with_name("gguf_cuda_hidden_device_gate.py")
SPEC = importlib.util.spec_from_file_location("cuda_hidden_device_gate", MODULE)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

ONE_PASS = (
    "running 1 test\n"
    "test cuda_build_without_a_visible_device_refuses_offload_but_serves_cpu ... ok\n"
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
)


class EvidencePassTests(unittest.TestCase):
    def test_real_marker_and_one_executed_test(self):
        self.assertTrue(gate.evidence_pass(gate.PASS_MARKER + "\n" + ONE_PASS, 0))

    def test_successful_exit_without_marker_is_not_qualified(self):
        self.assertFalse(gate.evidence_pass(ONE_PASS, 0))

    def test_conditional_skip_cannot_be_counted_as_pass(self):
        skipped = (
            "GPUQUAL hidden_device.qual_status=NOT_QUALIFIED (opt-in missing)\n"
            + ONE_PASS
        )
        self.assertFalse(gate.evidence_pass(skipped, 0))

    def test_no_executed_test_cannot_pass(self):
        log = (
            gate.PASS_MARKER + "\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 1 filtered out;\n"
        )
        self.assertFalse(gate.evidence_pass(log, 0))

    def test_failure_or_abort_cannot_pass(self):
        self.assertFalse(gate.evidence_pass(gate.PASS_MARKER + "\n" + ONE_PASS, 101))
        self.assertFalse(gate.evidence_pass(gate.PASS_MARKER + "\n" + ONE_PASS, 127))

    def test_unexpected_extra_ignored_test_does_not_pass(self):
        log = (
            gate.PASS_MARKER + "\n"
            "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured;\n"
        )
        self.assertFalse(gate.evidence_pass(log, 0))


class NativeBuildInputGuards(unittest.TestCase):
    def test_safe_minimal_environment_is_accepted(self):
        with mock.patch.object(gate, "checked", return_value=""):
            gate.validate_build_inputs(Path("unread"), {
                "GG_CORE_REQUIRE_REAL_MODEL": "1",
                "CARGO_NET_OFFLINE": "true",
                "CUDA_VISIBLE_DEVICES": "-1",
            })

    def test_unapproved_build_overrides_are_refused_without_echoing_values(self):
        for key in (
            "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER",
            "RUSTC", "CARGO_HOME", "CARGO_BUILD_TARGET", "CARGO_TARGET_DIR",
            "CARGO_PROFILE_RELEASE_LTO", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER",
        ):
            with self.subTest(key=key), mock.patch.object(gate, "checked", return_value=""):
                with self.assertRaisesRegex(RuntimeError, "compilation environment override") as error:
                    gate.validate_build_inputs(Path("unread"), {key: "sensitive-private-value"})
                self.assertIn(key, str(error.exception))
                self.assertNotIn("sensitive-private-value", str(error.exception))

    def test_git_ignored_rust_source_must_not_be_invisible(self):
        with tempfile.TemporaryDirectory(prefix="ggcore-hidden-build-input-") as folder:
            repo = Path(folder)
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            (repo / ".gitignore").write_text(
                "core-runtime/src/models/tier_synergy.rs\n", encoding="utf-8"
            )
            ignored = repo / "core-runtime/src/models/tier_synergy.rs"
            ignored.parent.mkdir(parents=True)
            ignored.write_text("// changed native build input\n", encoding="utf-8")
            # The ordinary clean-tree command does not reveal this file.
            status = gate.checked([
                "git", "status", "--porcelain", "--untracked-files=all",
                "--", "core-runtime/src/models/tier_synergy.rs",
            ], repo)
            self.assertEqual(status, "")
            with self.assertRaisesRegex(RuntimeError, "Ignored build input"):
                gate.validate_build_inputs(repo, {})

    def test_ignored_cargo_configuration_is_refused(self):
        with mock.patch.object(gate, "checked", return_value="core-runtime/.cargo/config.toml"):
            with self.assertRaisesRegex(RuntimeError, "Ignored build input"):
                gate.validate_build_inputs(Path("unread"), {})


if __name__ == "__main__":
    unittest.main()
