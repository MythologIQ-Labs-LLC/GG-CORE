"""Model-free failure-control tests for the Criterion regression gate."""
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import perf_gate


class PerfGateTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name) / "base"
        self.cur = Path(self.tmp.name) / "current"

    def add(self, root, name, median):
        dest = root / name / "new" / "estimates.json"
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(json.dumps({"median": {"point_estimate": median}}), encoding="utf-8")

    def run_gate(self, threshold="2.0"):
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            code = perf_gate.main(["perf_gate.py", str(self.base), str(self.cur), threshold])
        return code, stdout.getvalue()

    def test_empty_baseline_cache_miss_is_explicit_skip(self):
        code, output = self.run_gate()
        self.assertEqual(code, 0)
        self.assertIn("skip", output)

    def test_current_results_missing_must_fail(self):
        self.add(self.base, "latency/one", 1200)
        code, output = self.run_gate()
        self.assertEqual(code, 1)
        self.assertIn("MISSING", output)

    def test_one_missing_benchmark_must_fail(self):
        self.add(self.base, "latency/one", 1200)
        self.add(self.base, "latency/two", 1500)
        self.add(self.cur, "latency/one", 1200)
        self.assertEqual(self.run_gate()[0], 1)

    def test_existing_benchmark_passes_and_new_benchmark_allowed(self):
        self.add(self.base, "latency/one", 1200)
        self.add(self.cur, "latency/one", 1200)
        self.add(self.cur, "latency/new", 3000)
        code, output = self.run_gate()
        self.assertEqual(code, 0)
        self.assertIn("NEW", output)

    def test_single_benchmark_regression_cannot_normalize_itself(self):
        self.add(self.base, "latency/one", 1200)
        self.add(self.cur, "latency/one", 3600)
        self.assertEqual(self.run_gate()[0], 1)

    def test_two_benchmarks_cannot_hide_regression(self):
        for name, before, after in [("one", 1200, 3600), ("two", 1200, 1200)]:
            self.add(self.base, name, before)
            self.add(self.cur, name, after)
        self.assertEqual(self.run_gate()[0], 1)

    def test_uniform_shift_normalized_with_five_independent_benches(self):
        for i in range(5):
            self.add(self.base, f"group/{i}", 1500)
            self.add(self.cur, f"group/{i}", 2700)
        code, output = self.run_gate()
        self.assertEqual(code, 0)
        self.assertIn("normalizing", output)

    def test_outlier_fails_amid_uniform_runner_shift(self):
        for i in range(6):
            self.add(self.base, f"group/{i}", 1500)
            self.add(self.cur, f"group/{i}", 9000 if i == 5 else 2700)
        self.assertEqual(self.run_gate()[0], 1)

    def test_sub_microsecond_benchmark_is_diagnostic_only(self):
        self.add(self.base, "tiny", 900)
        self.add(self.cur, "tiny", 4500)
        code, output = self.run_gate()
        self.assertEqual(code, 0)
        self.assertIn("noisy", output)

    def test_invalid_baseline_or_current_median_fails_closed(self):
        for invalid in (0, -1, None, True, float("nan"), float("inf")):
            with self.subTest(invalid=invalid):
                self.add(self.base, "latency/one", 1000)
                self.add(self.cur, "latency/one", invalid)
                code, output = self.run_gate()
                self.assertEqual(code, 2)
                self.assertIn("INVALID INPUT", output)
                self.add(self.cur, "latency/one", 1000)
                self.add(self.base, "latency/one", invalid)
                self.assertEqual(self.run_gate()[0], 2)

    def test_invalid_threshold_fails_closed_even_with_cache_miss(self):
        for invalid in ("NaN", "inf", "0", "-1", "nonsense"):
            with self.subTest(invalid=invalid):
                self.assertEqual(self.run_gate(invalid)[0], 2)


if __name__ == "__main__":
    unittest.main()
