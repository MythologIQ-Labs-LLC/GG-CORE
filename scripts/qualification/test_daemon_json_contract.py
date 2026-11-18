"""Model-free security/meaning checks for authenticated daemon JSON inspectors."""

import json
import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
from daemon_json_contract import absent_model, healthy, ready_model  # noqa: E402

INSPECT = ROOT / "daemon_json_contract.py"


class DaemonJsonContract(unittest.TestCase):
    def run_inspect(self, args, payload):
        return subprocess.run(
            [sys.executable, str(INSPECT), *args],
            input=payload, text=True, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, check=False, timeout=10,
        )

    def test_only_explicit_healthy_object_qualifies(self):
        cases = [
            ({"health": "healthy"}, True),
            ({"health": "degraded"}, False),
            ({"health": "unhealthy"}, False),
            ({"health": "Healthy"}, False),
            ({"status": "healthy"}, False),
            ({"health": True}, False),
            ({"health": ["healthy"]}, False),
            ([{"health": "healthy"}], False),
            ("healthy", False),
        ]
        for value, expected in cases:
            with self.subTest(payload=value):
                self.assertIs(healthy(value), expected)
                self.assertEqual(
                    self.run_inspect(["health"], json.dumps(value)).returncode,
                    0 if expected else 1,
                )

    def test_ready_model_must_match_name_and_state_in_models_array(self):
        cases = [
            ({"models": [{"name": "qwen", "state": "ready"}]}, True),
            ({"models": [{"name": "other-qwen", "state": "ready"}]}, False),
            ({"models": [{"name": "qwen", "state": "loading"}]}, False),
            ({"models": [{"name": "phi", "state": "ready", "note": "qwen"}]}, False),
            ({"models": [{"name": "qwen", "state": "Ready"}]}, False),
            ({"models": {"name": "qwen", "state": "ready"}}, False),
            ({"models": [{"state": "ready", "name": None}]}, False),
            ({"models": ["qwen"]}, False),
            ({"models": []}, False),
            ({"note": "qwen"}, False),
            ([{"models": [{"name": "qwen", "state": "ready"}]}], False),
        ]
        for value, expected in cases:
            with self.subTest(payload=value):
                self.assertIs(ready_model(value, "qwen"), expected)
                self.assertEqual(
                    self.run_inspect(["ready-model", "qwen"], json.dumps(value)).returncode,
                    0 if expected else 1,
                )

    def test_absent_model_needs_valid_registry_and_exact_absence(self):
        cases = [
            ({"models": []}, True),
            ({"models": [{"name": "qwen", "state": "ready"}]}, True),
            ({"models": [{"name": "fake", "state": "ready"}]}, False),
            ({"models": [{"name": "fake", "state": "loading"}]}, False),
            ({"models": [{"name": "fake", "state": "loaded_unverified"}]}, False),
            ({"models": [{"name": "other-fake", "state": "ready"}]}, True),
            ({"models": [{"name": "qwen", "state": "ready"}, {"name": "fake", "state": "error"}]}, False),
            ({"models": [{"state": "ready"}]}, False),
            ({"models": ["qwen"]}, False),
            ({"models": {"name": "qwen"}}, False),
            ({"status": "ok"}, False),
            ([{"models": []}], False),
            ("", False),
        ]
        for value, expected in cases:
            with self.subTest(payload=value):
                self.assertIs(absent_model(value, "fake"), expected)
                self.assertEqual(
                    self.run_inspect(["absent-model", "fake"], json.dumps(value)).returncode,
                    0 if expected else 1,
                )

    def test_malformed_data_and_bad_usage_fail_closed(self):
        for args, payload in [
            (["health"], ""),
            (["health"], '{"health":'),
            (["ready-model", "qwen"], "not json"),
            (["ready-model"], '{"models":[]}'),
            (["ready-model", ""], '{"models":[]}'),
            (["absent-model", ""], '{"models":[]}'),
            (["absent-model", "fake"], "not json"),
            (["health", "extra"], '{"health":"healthy"}'),
            (["unknown"], '{"health":"healthy"}'),
        ]:
            with self.subTest(args=args, payload=payload):
                self.assertNotEqual(self.run_inspect(args, payload).returncode, 0)


if __name__ == "__main__":
    unittest.main()
