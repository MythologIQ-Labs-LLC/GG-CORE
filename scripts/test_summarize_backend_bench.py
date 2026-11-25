"""Model-free adversarial tests for non-comparative evidence matrix."""
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from summarize_backend_bench import main, read_receipts, summarize, safe_median

BASE = {"schema_version": 1, "backend": "gguf", "task": "generation",
        "status": "verified", "source_commit": "a"*40,
        "host_id": "isolated-1", "os": "linux", "model_sha256": "b"*64,
        "fixture_sha256": "c"*64, "latency_ms": [14, 11, 13],
        "correctness_passed": True, "decode_tokens_per_second": 7.0}

class MatrixTests(unittest.TestCase):
    def test_full_grid_and_single_lane_metric(self):
        out = summarize([BASE])
        self.assertEqual(len(out["capabilities"]), 9)
        gguf = [x for x in out["capabilities"] if x["backend"]=="gguf" and x["task"]=="generation"][0]
        self.assertEqual(gguf["reported_runs"][0]["reported_latency"]["median_ms"],13)
        self.assertEqual(sum(x["receipt_count"] for x in out["capabilities"]),1)
        self.assertIn("prohibited",out["cross_backend_ranking"])

    def test_different_model_hosts_never_pooled(self):
        second = dict(BASE, host_id="isolated-2", model_sha256="d"*64,
                      latency_ms=[500, 1000], decode_tokens_per_second=9.0)
        out = summarize([BASE,second])
        cell = [c for c in out["capabilities"] if c["backend"]=="gguf" and c["task"]=="generation"][0]
        self.assertEqual(cell["receipt_count"],2)
        self.assertEqual([r["reported_latency"]["samples"] for r in cell["reported_runs"]],[3,2])
        self.assertEqual([r["reported_latency"]["median_ms"] for r in cell["reported_runs"]],[13,750])

    def test_unsupported_omits_false_model_and_throughput(self):
        data = {k:v for k,v in BASE.items() if k not in
                ("latency_ms","correctness_passed","decode_tokens_per_second","model_sha256","fixture_sha256")}
        data.update(backend="hologram",status="unsupported",reason="no model")
        out=summarize([data])
        record=[c for c in out["capabilities"] if c["backend"]=="hologram" and c["task"]=="generation"][0]["reported_runs"][0]
        self.assertNotIn("model_sha256",record)
        self.assertNotIn("reported_latency",record)
        self.assertTrue(record["reason_recorded"])

    def test_even_median_no_overflow(self):
        self.assertEqual(safe_median([1e308,1e308]),1e308)

    def test_never_echo_sensitive_extra_fields(self):
        data=dict(BASE,prompt="private-text",token="super-secret",response="hidden")
        self.assertNotIn("private-text",json.dumps(summarize([data])))
        self.assertNotIn("super-secret",json.dumps(summarize([data])))

    def test_cli_success_and_error_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)/"runs.jsonl"
            p.write_text(json.dumps(BASE)+"\n",encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(main([str(p)]),0)
            self.assertEqual(json.loads(output.getvalue())["kind"],"backend_benchmark_matrix")
            for text in ('{"schema_version":1,"schema_version":1}\n',
                         json.dumps(dict(BASE,backend=[]))+"\n",'\n'):
                p.write_text(text,encoding="utf-8")
                with contextlib.redirect_stderr(io.StringIO()),contextlib.redirect_stdout(io.StringIO()) as out:
                    self.assertEqual(main([str(p)]),1)
                self.assertEqual(out.getvalue(),"")

    def test_one_invalid_row_invalidates_entire_matrix(self):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)/"invalid.jsonl"
            p.write_text(json.dumps(BASE)+"\n"+json.dumps(dict(BASE,backend=[]))+"\n",encoding="utf-8")
            with self.assertRaisesRegex(ValueError,"line 2"):
                read_receipts(p)

    def test_deeply_nested_json_is_refused_without_traceback(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory) / "deep.jsonl"
            p.write_text("[" * 15000 + "0" + "]" * 15000 + "\n", encoding="utf-8")
            with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(main([str(p)]), 1)
            self.assertEqual(output.getvalue(), "")

    def test_input_file_size_ceiling(self):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)/"huge.jsonl"
            p.write_bytes(b" "*(2*1024*1024+1))
            with self.assertRaisesRegex(ValueError,"size ceiling"):
                read_receipts(p)

if __name__=="__main__":
    unittest.main()
