"""Model-free tests for legacy GG-CORE smoke script truthfulness.

Synthetic CLI/fixture is ONLY a test of failure reporting, never native model
qualification. Does not download files, build Rust, or contact any daemon.
"""

import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BASH = ROOT / "scripts" / "e2e-proof.sh"
PS = ROOT / "scripts" / "e2e-proof.ps1"

FAKE_CLI = r"""#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  version) echo '{"version":"synthetic-test-only"}' ;;
  status)
    [[ "${FAKE_MODE:-ok}" != "no-daemon" ]] || exit 7
    case "${FAKE_MODE:-ok}" in
      unhealthy-daemon) echo '{"health":"unhealthy"}' ;;
      degraded-daemon) echo '{"health":"degraded"}' ;;
      missing-health) echo '{"status":"healthy"}' ;;
      invalid-status) echo '{"health":' ;;
      *) echo '{"health":"healthy"}' ;;
    esac
    ;;
  models)
    case "${FAKE_MODE:-ok}" in
      no-model) echo '{"models":[]}' ;;
      substring-name) echo '{"models":[{"name":"other-qwen","state":"ready"}]}' ;;
      loading-model) echo '{"models":[{"name":"qwen","state":"loading"}]}' ;;
      other-field) echo '{"models":[{"name":"phi","state":"ready","note":"qwen"}]}' ;;
      bad-json) echo '{"models":[' ;;
      *) echo '{"models":[{"name":"qwen","state":"ready"}]}' ;;
    esac
    ;;
  infer)
    [[ "${FAKE_MODE:-ok}" != "infer-fail" ]] || exit 9
    for arg in "$@"; do
      if [[ "$arg" == "--stream" ]]; then
        [[ "${FAKE_MODE:-ok}" != "stream-fail" ]] || exit 11
        echo "one two three four five"
        exit 0
      fi
    done
    [[ "${FAKE_MODE:-ok}" != "wrong-answer" ]] || { echo "5"; exit 0; }
    echo "4"
    ;;
  *) exit 15 ;;
esac
"""


class SmokeContract(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory(prefix="gg-core-smoke-contract-")
        self.addCleanup(self.work.cleanup)
        root = pathlib.Path(self.work.name)
        self.fixture_dir = root / "fixture with spaces"
        self.fixture_dir.mkdir()
        (self.fixture_dir / "qwen2.5-0.5b-instruct-q4_k_m.gguf").write_bytes(
            b"GGUFsynthetic-not-a-real-model"
        )
        self.cli = root / "gg-core-cli"
        self.cli.write_text(FAKE_CLI, encoding="utf-8")
        self.cli.chmod(0o755)

    def invoke(self, mode="ok", token="synthetic-token", fixture=True):
        env = os.environ.copy()
        env.update(
            {
                "GG_CORE_CLI": str(self.cli),
                "SKIP_BUILD": "true",
                "CORE_AUTH_TOKEN": token,
                "MODELS_DIR": str(self.fixture_dir if fixture else
                                  self.fixture_dir / "missing"),
                "FAKE_MODE": mode,
            }
        )
        return subprocess.run(
            ["bash", str(BASH)], cwd=self.work.name, env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=15, check=False,
        )

    def test_bash_syntax(self):
        result = subprocess.run(
            ["bash", "-n", str(BASH)], capture_output=True, text=True, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_success_is_narrow_and_discloses_unrun_gates(self):
        run = self.invoke()
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertIn("SMOKE PASS", run.stdout)
        self.assertIn("exact ready model", run.stdout)
        self.assertIn("RELEASE HOLD", run.stdout)
        self.assertIn("NOT RUN: active cancellation", run.stdout)
        self.assertIn("NOT VERIFIED: metric deltas", run.stdout)
        self.assertNotIn("RELEASE PASS", run.stdout)

    def test_missing_auth_or_fixture_is_refused(self):
        for params in ({"token": ""}, {"fixture": False}):
            with self.subTest(params=params):
                self.assertNotEqual(self.invoke(**params).returncode, 0)

    def test_unusable_daemon_or_unverified_model_fails(self):
        for mode in (
            "no-daemon", "unhealthy-daemon", "degraded-daemon",
            "missing-health", "invalid-status", "no-model", "substring-name",
            "loading-model", "other-field", "bad-json",
        ):
            with self.subTest(mode=mode):
                self.assertNotEqual(self.invoke(mode=mode).returncode, 0)

    def test_failed_or_incorrect_inference_never_passes(self):
        for mode in ("infer-fail", "wrong-answer", "stream-fail"):
            with self.subTest(mode=mode):
                run = self.invoke(mode=mode)
                self.assertNotEqual(run.returncode, 0)
                self.assertNotIn("SMOKE PASS", run.stdout)

    def test_lfs_pointer_refused_before_stub_cli(self):
        model = self.fixture_dir / "qwen2.5-0.5b-instruct-q4_k_m.gguf"
        model.write_text("version https://git-lfs.github.com/spec/v1")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not GGUF", result.stderr)

    def test_rc_helpers_remain_resolvable_after_separate_fixture_cd(self):
        """Run the real shell path-binding lines in a separate model directory.

        This is a model-free path regression, not a daemon/package test.
        """
        source = (ROOT / "scripts" / "rc-acceptance.sh").read_text(encoding="utf-8")
        bindings = [line for line in source.splitlines() if line.startswith(
            ("SCRIPT_DIR=", "INSPECT=", "IPC_PROBE=")
        )]
        self.assertEqual(len(bindings), 3)
        self.assertLess(source.index("SCRIPT_DIR="), source.index('cd "$BASE" || exit 1'))
        self.assertIn('with_timeout 420 "$PYTHON" "$IPC_PROBE"', source)
        self.assertNotIn('"$BASE/scripts/rc-ipc-control-acceptance.py"', source)
        with tempfile.TemporaryDirectory(prefix="gg-core-rc-paths-") as folder:
            root = pathlib.Path(folder)
            source_dir = root / "script directory with spaces"
            (source_dir / "qualification").mkdir(parents=True)
            (source_dir / "qualification" / "daemon_json_contract.py").touch()
            (source_dir / "rc-ipc-control-acceptance.py").touch()
            fixture = root / "independent model fixture"
            fixture.mkdir()
            wrapper = source_dir / "check.sh"
            wrapper.write_text(
                "#!/usr/bin/env bash\\n" + "\\n".join(bindings) +
                '\\ncd "$1" || exit 40\\n' +
                '[ -f "$INSPECT" ] && [ -f "$IPC_PROBE" ] || exit 41\\n',
                encoding="utf-8",
            )
            result = subprocess.run(
                ["bash", str(wrapper), str(fixture)], cwd=root,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True, timeout=10, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_scripts_cannot_silently_fetch_models_or_certify_release(self):
        bash = BASH.read_text(encoding="utf-8")
        ps = PS.read_text(encoding="utf-8")
        for source in (bash, ps):
            with self.subTest(script=source[:40]):
                self.assertNotIn("download-models", source)
                self.assertIn("--locked --offline", source)
                self.assertIn("CORE_AUTH_TOKEN", source)
                self.assertIn("NOT RUN:", source)
                self.assertIn("RELEASE HOLD:", source)
                self.assertNotIn("E2E requirements satisfied", source)
        self.assertIn("SMOKE PASS:", ps)


if __name__ == "__main__":
    unittest.main()
