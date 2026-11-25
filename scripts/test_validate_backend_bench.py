"""Model-free negative controls for three-backend benchmark evidence."""
import copy
import unittest

from validate_backend_bench import validate


BASE = dict(
    schema_version=1, backend="gguf", task="generation", status="verified",
    source_commit="a" * 40, host_id="isolated-win-1", os="Windows 11",
    model_sha256="b" * 64, fixture_sha256="c" * 64,
    latency_ms=[20, 23, 18], correctness_passed=True,
    decode_tokens_per_second=25,
)


class ReceiptTests(unittest.TestCase):
    def test_valid_measured(self):
        self.assertEqual(validate(BASE), [])

    def test_valid_unsupported(self):
        row = {k: v for k, v in BASE.items()
               if k not in ("latency_ms", "correctness_passed", "decode_tokens_per_second")}
        row.update(backend="onnx", task="generation",
                   status="unsupported", reason="no qualified generation")
        self.assertEqual(validate(row), [])

    def test_no_invented_percentiles(self):
        self.assertTrue(any("percentile" in x for x in validate(dict(BASE, p99_ms=100))))

    def test_non_generation_tokens_refused(self):
        self.assertTrue(any("non-generation" in x for x in validate(dict(BASE, task="embedding"))))

    def test_nonfinite_negative_empty_samples_refused(self):
        for samples in ([], [float("nan")], [float("inf")], [-1], ["12"]):
            with self.subTest(samples=samples):
                self.assertTrue(validate(dict(BASE, latency_ms=samples)))

    def test_false_correctness_refused(self):
        self.assertTrue(validate(dict(BASE, correctness_passed=False)))

    def test_missing_provenance_refused(self):
        row = copy.deepcopy(BASE)
        del row["model_sha256"]
        self.assertTrue(validate(row))

    def test_blocked_must_not_publish_samples(self):
        self.assertTrue(validate(dict(BASE, status="blocked", reason="no model")))


if __name__ == "__main__":
    unittest.main()
