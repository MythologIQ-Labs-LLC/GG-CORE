"""Model-free, cross-platform regressions for the Windows evidence collector."""

import importlib.util
import json
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
            if label != "fmt":
                # cargo-deny is not governed by CARGO_NET_OFFLINE alone:
                # its advisory database fetch must be explicitly disabled.
                self.assertIn("--locked", command, label)
                self.assertIn("--offline", command, label)
        self.assertEqual(
            steps["cargo_deny"],
            ["cargo", "deny", "--offline", "--locked", "check"],
        )
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
                    native.validate_checkout(repo, "a" * 40, native.APPROVED_MODEL_SHA256)

    def test_git_head_and_checksum_are_mandatory(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(ValueError):
                native.validate_checkout(pathlib.Path(root), "bad", "b" * 64)
            with mock.patch.object(native, "checked_stdout", return_value="b" * 40):
                with self.assertRaisesRegex(RuntimeError, "differs"):
                    native.validate_checkout(pathlib.Path(root), "a" * 40, native.APPROVED_MODEL_SHA256)

    def test_untracked_source_fails_exact_head_even_when_commit_matches(self):
        with tempfile.TemporaryDirectory() as root:
            with mock.patch.object(native, "checked_stdout", side_effect=["a" * 40, "?? core-runtime/src/sidecar.rs"]):
                with self.assertRaisesRegex(RuntimeError, "Uncommitted or untracked source"):
                    native.validate_checkout(pathlib.Path(root), "a" * 40, native.APPROVED_MODEL_SHA256)

    def test_model_identity_is_pinned_to_the_release_workflow(self):
        import re
        release_workflow = (PATH.parents[2] / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        advertised = re.findall(
            r"^\s+MODEL_SHA256:\s*([0-9a-f]{64})\s*$",
            release_workflow,
            re.MULTILINE,
        )
        self.assertEqual(advertised, [native.APPROVED_MODEL_SHA256])
        self.assertEqual(len(native.APPROVED_MODEL_SHA256), 64)

    def test_matching_but_unapproved_gguf_checksum_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            repo = pathlib.Path(root)
            model = repo / native.MODEL_RELATIVE
            model.parent.mkdir(parents=True)
            model.write_bytes(b"GGUF" + b"unapproved model payload")
            alternative_sha = native.digest(model)
            self.assertNotEqual(alternative_sha, native.APPROVED_MODEL_SHA256)
            with mock.patch.object(native, "checked_stdout", side_effect=["a" * 40, ""]):
                with self.assertRaisesRegex(RuntimeError, "not the approved release"):
                    native.validate_checkout(repo, "a" * 40, alternative_sha)

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
                    native.collect_artifacts(core, core / "target", {"source_sha": "a" * 40})
            with mock.patch.object(native, "checked_stdout", return_value=json.dumps(
                {"commit": "a" * 40, "features": ["gguf"]}
            )):
                with self.assertRaisesRegex(RuntimeError, "feature provenance differs"):
                    native.collect_artifacts(core, core / "target", {"source_sha": "a" * 40})

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

    def test_stale_default_target_executables_are_not_accepted(self):
        with tempfile.TemporaryDirectory() as root:
            core = pathlib.Path(root)
            stale = core / "target/debug/deps"
            stale.mkdir(parents=True)
            (stale / "gg_core-oldhead.exe").write_bytes(b"old run")
            # An isolated run must not borrow existing cargo output, even
            # when an old test executable is present under the default path.
            with self.assertRaisesRegex(RuntimeError, "No compiled Windows test binaries"):
                native.collect_artifacts(core, core / "fresh-attempt/cargo-target", {})

    def test_existing_receipt_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as root:
            destination = pathlib.Path(root) / "evidence"
            destination.mkdir()
            receipt = destination / "receipt.json"
            receipt.write_text('{"status":"OLD_VERIFIED_RUN"}', encoding="utf-8")
            result = native.main([
                "--expected-head", "a" * 40,
                "--model-sha256", native.APPROVED_MODEL_SHA256,
                "--evidence-dir", str(destination),
            ])
            self.assertEqual(result, 1)
            self.assertEqual(receipt.read_text(encoding="utf-8"),
                             '{"status":"OLD_VERIFIED_RUN"}')

    def test_cargo_output_is_always_isolated_from_inherited_target_dir(self):
        with tempfile.TemporaryDirectory() as root:
            destination = pathlib.Path(root) / "evidence"
            observed = []
            with (
                mock.patch.object(native, "require_offline_windows"),
                mock.patch.object(native, "validate_checkout",
                                  return_value=native.APPROVED_MODEL_SHA256),
                mock.patch.object(native, "checked_stdout", return_value="tool version"),
                mock.patch.object(native, "collect_artifacts"),
                mock.patch.object(native.inputs, "require_controlled_build", return_value={}),
                mock.patch.object(native.inputs, "toolchain_identity", return_value={}),
                mock.patch.object(native, "run_step",
                                  side_effect=lambda label, argv, cwd, env, evidence, report:
                                      observed.append((env["CARGO_TARGET_DIR"], label))),
                mock.patch.dict(native.os.environ, {"CARGO_TARGET_DIR": "C:/stale-unsafe-build"}),
            ):
                code = native.main([
                    "--expected-head", "a" * 40,
                    "--model-sha256", native.APPROVED_MODEL_SHA256,
                    "--evidence-dir", str(destination),
                ])
            self.assertEqual(code, 0)
            self.assertEqual(len(observed), len(native.command_matrix()))
            expected = str(destination.resolve() / "cargo-target")
            self.assertEqual({path for path, _ in observed}, {expected})
            receipt = json.loads((destination / "receipt.json").read_text())
            self.assertEqual(receipt["cargo_target_dir"], expected)
            self.assertEqual(receipt["status"],
                             "SOURCE_AND_NATIVE_TEST_MATRIX_PASS__RELEASE_STILL_HOLD")

    def test_ignored_compile_input_refuses_even_with_matching_head(self):
        import subprocess
        with tempfile.TemporaryDirectory() as root:
            repo = pathlib.Path(root)
            git = lambda *a: subprocess.check_output(
                ["git", "-c", "user.name=t", "-c", "user.email=t@t", *a], cwd=repo, text=True).strip()
            git("init", "-q")
            (repo / ".gitignore").write_text("src/private_module.rs\n")
            git("add", ".gitignore"), git("commit", "-q", "-m", "base"), (repo / "src").mkdir()
            (repo / "src/private_module.rs").write_text("pub fn injected() {}\n")
            with self.assertRaisesRegex(RuntimeError, "Ignored build inputs.*private_module"):
                native.validate_checkout(repo, git("rev-parse", "HEAD"), native.APPROVED_MODEL_SHA256)

    def test_injected_build_overrides_refuse_and_gate_settings_are_allowed(self):
        for name in ("RUSTFLAGS", "RUSTC_WRAPPER", "rustc_workspace_wrapper", "_CL_",
                     "CARGO_PROFILE_DEV_OPT_LEVEL", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER",
                     "GGML_CUDA", "CMAKE_CUDA_ARCHITECTURES"):
            self.assertEqual(native.inputs.forbidden_overrides({name: "x"}), [name])
            with self.assertRaisesRegex(RuntimeError, "environment overrides"):
                native.inputs.require_controlled_build(pathlib.Path("."), {name: "x"})
        allowed = {"CARGO_TARGET_DIR": "t", "CMAKE_GENERATOR": "Ninja", "CARGO_HOME": "h",
                   "CARGO_NET_OFFLINE": "true", "PATH": "p", "LIBCLANG_PATH": "l"}
        self.assertEqual(native.inputs.forbidden_overrides(allowed), [])

    def test_cargo_config_outside_checkout_refuses(self):
        with tempfile.TemporaryDirectory() as root:
            (home := pathlib.Path(root) / "cargo-home").mkdir()
            (home / "config.toml").write_text("[build]\nrustflags = ['-Cdebug-assertions=off']\n")
            with self.assertRaisesRegex(RuntimeError, "Cargo config outside the checkout"):
                native.inputs.require_controlled_build(pathlib.Path(root) / "repo",
                                                       {"CARGO_HOME": str(home)})

    def test_injected_override_writes_hold_receipt_and_runs_nothing(self):
        with tempfile.TemporaryDirectory() as root:
            destination = pathlib.Path(root) / "evidence"
            with (mock.patch.object(native, "require_offline_windows"),
                  mock.patch.object(native, "validate_checkout", return_value="m"),
                  mock.patch.object(native, "run_step") as run_step,
                  mock.patch.dict(native.os.environ, {"RUSTC_WRAPPER": "sccache"})):
                code = native.main(["--expected-head", "a" * 40, "--model-sha256",
                                    native.APPROVED_MODEL_SHA256, "--evidence-dir", str(destination)])
            receipt = json.loads((destination / "receipt.json").read_text())
            self.assertEqual((code, receipt["status"], run_step.called), (1, "HOLD", False))
            self.assertIn("RUSTC_WRAPPER", receipt["error"])

    def test_no_windows_test_binaries_never_passes(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(RuntimeError, "No compiled Windows test binaries"):
                native.collect_artifacts(pathlib.Path(root), pathlib.Path(root) / "target", {})


if __name__ == "__main__":
    unittest.main()
