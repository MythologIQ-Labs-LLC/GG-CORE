"""Deterministic negative controls for model-free benchmark evidence validation."""
import copy
import json
import tempfile
import unittest
from pathlib import Path

from validate_backend_bench import main, validate

BASE = dict(schema_version=1, backend="gguf", task="generation", status="verified",
            source_commit="a" * 40, host_id="isolated-win-1", os="Windows 11",
            model_sha256="b" * 64, fixture_sha256="c" * 64,
            latency_ms=[20, 23, 18], correctness_passed=True,
            decode_tokens_per_second=25)


class ReceiptTests(unittest.TestCase):
    def test_valid_measured(self):
        self.assertEqual(validate(BASE), [])

    def test_unsupported_without_invented_model_identity(self):
        row = {k: v for k, v in BASE.items() if k not in
               ("model_sha256", "fixture_sha256", "latency_ms", "correctness_passed", "decode_tokens_per_second")}
        row.update(backend="hologram", task="generation", status="unsupported", reason="no compatible real model")
        self.assertEqual(validate(row), [])

    def test_percentiles_refused(self):
        self.assertTrue(any("percentile" in err for err in validate(dict(BASE, p99_ms=10))))

    def test_non_generation_tokens_refused(self):
        self.assertTrue(any("non-generation" in err for err in validate(dict(BASE, task="embedding"))))

    def test_nonfinite_negative_empty_samples_refused(self):
        for samples in ([], [float("nan")], [float("inf")], [-1], ["12"], [True]):
            with self.subTest(samples=samples):
                self.assertTrue(validate(dict(BASE, latency_ms=samples)))

    def test_false_correctness_refused(self):
        self.assertTrue(validate(dict(BASE, correctness_passed=False)))

    def test_missing_measured_provenance_refused(self):
        row = copy.deepcopy(BASE)
        del row["model_sha256"]
        self.assertTrue(validate(row))

    def test_blocked_must_not_publish_samples(self):
        self.assertTrue(validate(dict(BASE, status="blocked", reason="no model")))

    def test_unmeasured_must_not_claim_correctness(self):
        row = dict(BASE, status="unmeasured", reason="pending")
        row.pop("latency_ms")
        row.pop("decode_tokens_per_second")
        self.assertTrue(validate(row))

    def test_bool_schema_and_throughput_refused(self):
        self.assertTrue(validate(dict(BASE, schema_version=True)))
        self.assertTrue(validate(dict(BASE, decode_tokens_per_second=True)))

    def test_malformed_enum_types_return_errors_without_crashing(self):
        for field in ("backend", "task", "status"):
            for malformed in ([], {}, {"field": "value"}, 1, False, None):
                with self.subTest(field=field, malformed=malformed):
                    errors = validate(dict(BASE, **{field: malformed}))
                    self.assertIn(f"invalid {field}", errors)

    def test_cli_continues_after_malformed_enum_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "malformed-enums.jsonl"
            rows = [dict(BASE, backend=[]), dict(BASE, task={}), dict(BASE, status=[]), BASE]
            path.write_text("".join(json.dumps(row) + "\\n" for row in rows), encoding="utf-8")
            self.assertEqual(main([str(path)]), 1)

    def test_cli_accepts_valid_jsonl(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "valid.jsonl"
            path.write_text(json.dumps(BASE) + "\n", encoding="utf-8")
            self.assertEqual(main([str(path)]), 0)

    def test_cli_rejects_duplicate_json_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "duplicate.jsonl"
            path.write_text('{"schema_version":1,"schema_version":1}\n', encoding="utf-8")
            self.assertEqual(main([str(path)]), 1)

    def test_cli_rejects_missing_file(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(main([str(Path(directory) / "missing.jsonl")]), 1)

    def test_cli_rejects_empty_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "empty.jsonl"
            path.write_text("\n", encoding="utf-8")
            self.assertEqual(main([str(path)]), 1)


if __name__ == "__main__":
    unittest.main()
