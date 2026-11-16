"""Pure acceptance-probe regressions. No models, daemon, or network needed."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import typed_onnx_packaged_acceptance as a


class TestAcceptanceProbe(unittest.TestCase):
    def test_hash_and_stage_containment(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            model = root / "model.onnx"
            model.write_bytes(b"sample-weights")
            digest = hashlib.sha256(b"sample-weights").hexdigest()
            self.assertEqual(a.checked_file(root, "model.onnx", digest), model)
            with self.assertRaises(a.AcceptanceError):
                a.checked_file(root, "../outside", digest)
            with self.assertRaises(a.AcceptanceError):
                a.checked_file(root, "model.onnx", "0" * 64)
            with self.assertRaises(a.AcceptanceError):
                a.checked_file(root, "model.onnx", "INVALID")

    def test_embedding_contract_refuses_wrong_or_nonfinite_values(self):
        v = a.finite_vector({"kind": "embedding", "dimensions": 384,
                             "vector": [0.25] * 384})
        self.assertEqual(len(v), 384)
        for bad in (float("nan"), float("inf"), True, "not a number"):
            vector = [0.25] * 383 + [bad]
            with self.assertRaises(a.AcceptanceError):
                a.finite_vector({"kind": "embedding", "dimensions": 384, "vector": vector})
        with self.assertRaises(a.AcceptanceError):
            a.finite_vector({"kind": "embedding", "dimensions": 383,
                             "vector": [0.25] * 383})

    def test_semantic_similarity_and_classification(self):
        self.assertAlmostEqual(a.cosine([1.0, 0.0], [1.0, 0.0]), 1.0)
        self.assertLess(a.cosine([1.0, 0.0], [0.0, 1.0]), 0.01)
        with self.assertRaises(a.AcceptanceError):
            a.cosine([0.0, 0.0], [1.0, 0.0])
        a.expected_classification({"kind": "classification", "label": "POSITIVE",
                                   "confidence": 0.97}, "POSITIVE")
        for wrong in (None, True, float("nan"), 1.2):
            with self.assertRaises(a.AcceptanceError):
                a.expected_classification({"kind": "classification", "label": "POSITIVE",
                                           "confidence": wrong}, "POSITIVE")
        with self.assertRaises(a.AcceptanceError):
            a.expected_classification({"kind": "classification", "label": "NEGATIVE",
                                       "confidence": 0.91}, "POSITIVE")

    def test_fail_closed_receipt_with_bad_spec(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            spec = root / "bad.json"
            spec.write_text(json.dumps({"source_commit": "bad"}), encoding="utf-8")
            out = root / "receipt.json"
            self.assertEqual(a.main(["--stage", t, "--spec", str(spec), "--receipt", str(out)]), 1)
            receipt = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(receipt["status"], "FAIL")
            self.assertEqual(receipt["error_code"], "invalid-source-commit")
            self.assertNotIn("auth", str(receipt).lower())


if __name__ == "__main__":
    unittest.main()
