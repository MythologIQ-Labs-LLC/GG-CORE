"""Fail-closed, model-free assertions for #114 conformance inventory."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

PATH = Path(__file__).with_name("conformance_receipts.py")
SPEC = importlib.util.spec_from_file_location("conformance_receipts", PATH)
assert SPEC and SPEC.loader
mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(mod)
MANIFEST = json.loads(Path(__file__).with_name("conformance_inventory.json").read_text())
SOURCE = "a" * 40


def assertion(case, status="PASS", **kwargs):
    return {
        "id": case["id"], "source_sha": SOURCE, "status": status,
        "platform": "linux" if case["platform"] == "any" else case["platform"],
        "features": case["features"], **kwargs,
    }


class ConformanceReceiptTests(unittest.TestCase):
    def test_inventory_has_no_implicit_pass_or_duplicate(self):
        cases = mod.validate_manifest(MANIFEST)
        self.assertGreaterEqual(len(cases), 7)
        self.assertTrue(any(c["tier"] == "installed" for c in cases.values()))
        self.assertTrue(any(c["model_required"] for c in cases.values()))
        report = mod.report(MANIFEST, [], SOURCE)
        self.assertEqual(report["counts"]["NOT_RUN"], len(cases))
        self.assertEqual(report["counts"]["RECORDED_PASS"], 0)
        self.assertEqual((report["status"], report["release_authorized"],
                          report["evidence_attested"]), ("HOLD", False, False))

    def test_source_evidence_is_recorded_not_independently_verified(self):
        case = MANIFEST["cases"][0]
        r = mod.report(MANIFEST, [assertion(case, evidence_ref="ci-source-log")], SOURCE)
        self.assertEqual(r["counts"]["RECORDED_PASS"], 1)
        self.assertEqual(r["status"], "HOLD")
        self.assertEqual(r["cases"][0]["interface"], case["interface"])
        self.assertFalse(r["release_authorized"])
        self.assertFalse(r["evidence_attested"])

    def test_stale_head_unknown_id_and_duplicate_evidence_refused(self):
        case = MANIFEST["cases"][0]
        item = assertion(case, evidence_ref="ci-source-log")
        for bad in [
            dict(item, source_sha="b" * 40),
            dict(item, id="nonexistent"),
            dict(item, status="green"),
            dict(item, features=["gguf"]),
        ]:
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    mod.report(MANIFEST, [bad], SOURCE)
        with self.assertRaisesRegex(ValueError, "duplicate evidence"):
            mod.report(MANIFEST, [item, item], SOURCE)

    def test_installed_pass_requires_real_binary_and_matching_digest(self):
        case = next(c for c in MANIFEST["cases"] if c["tier"] == "installed")
        item = assertion(case, evidence_ref="local-installed-receipt")
        with self.assertRaisesRegex(ValueError, "binary"):
            mod.report(MANIFEST, [item], SOURCE)
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp, "dummy.exe")
            binary.write_bytes(b"isolated test bytes only")
            item.update(binary_path=str(binary),
                        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            if case["model_required"]:
                model = Path(tmp, "fixture.gguf")
                model.write_bytes(b"fake test fixture")
                item.update(model_path=str(model),
                            model_sha256=hashlib.sha256(model.read_bytes()).hexdigest())
                if case["tier"] == "installed":
                    with self.assertRaisesRegex(ValueError, "license review"):
                        mod.report(MANIFEST, [item], SOURCE)
                    item["model_license_ref"] = "owner-reviewed-model-fixture-license"
            r = mod.report(MANIFEST, [item], SOURCE)
            self.assertEqual(r["counts"]["RECORDED_PASS"], 1)
            self.assertEqual(r["status"], "HOLD")
            entry = next(x for x in r["cases"] if x["id"] == case["id"])
            self.assertEqual(entry["binary_sha256"], item["binary_sha256"])
            self.assertEqual(entry["model_sha256"], item.get("model_sha256"))
            binary.write_bytes(b"tampered after receipt")
            with self.assertRaisesRegex(ValueError, "content hash mismatch"):
                mod.report(MANIFEST, [item], SOURCE)

    def test_model_required_refuses_missing_and_forged_model(self):
        case = next(c for c in MANIFEST["cases"]
                    if c["tier"] == "source" and c["model_required"])
        item = assertion(case, evidence_ref="model-fixture-source-test")
        with self.assertRaisesRegex(ValueError, "model"):
            mod.report(MANIFEST, [item], SOURCE)
        with tempfile.TemporaryDirectory() as tmp:
            model = Path(tmp, "fixture.bin")
            model.write_bytes(b"fixture")
            item.update(model_path=str(model), model_sha256="0" * 64)
            with self.assertRaisesRegex(ValueError, "content hash mismatch"):
                mod.report(MANIFEST, [item], SOURCE)

    def test_unsupported_requires_explicit_owner_reference_and_never_passes(self):
        case = MANIFEST["cases"][0]
        bad = assertion(case, status="UNSUPPORTED")
        with self.assertRaisesRegex(ValueError, "owner disposition"):
            mod.report(MANIFEST, [bad], SOURCE)
        good = dict(bad, owner_decision_ref="owner-reviewed-scope-record")
        r = mod.report(MANIFEST, [good], SOURCE)
        self.assertEqual(r["counts"]["UNSUPPORTED"], 1)
        self.assertEqual(r["counts"]["RECORDED_PASS"], 0)
        self.assertEqual(r["status"], "HOLD")

    def test_wrong_platform_and_invalid_manifest_fail_closed(self):
        case = next(c for c in MANIFEST["cases"] if c["platform"] == "windows")
        with self.assertRaisesRegex(ValueError, "wrong platform"):
            mod.report(MANIFEST, [assertion(case, platform="linux", status="NOT_RUN")], SOURCE)
        with self.assertRaisesRegex(ValueError, "duplicate case ID"):
            mod.validate_manifest({"schema_version": 1,
                                   "cases": [MANIFEST["cases"][0]] * 2})
        with self.assertRaises(ValueError):
            mod.report(MANIFEST, [], "short")

    def test_cli_strict_fails_unrun_without_fabricating_green(self):
        base = [sys.executable, "-I", str(PATH), "--source-sha", SOURCE]
        call = subprocess.run(base, capture_output=True, text=True, check=False)
        self.assertEqual(call.returncode, 0, call.stderr)
        receipt = json.loads(call.stdout)
        self.assertEqual(receipt["status"], "HOLD")
        strict = subprocess.run(base + ["--fail-on-incomplete"],
                                capture_output=True, text=True, check=False)
        self.assertEqual(strict.returncode, 1, strict.stderr)
        self.assertEqual(json.loads(strict.stdout)["status"], "HOLD")


if __name__ == "__main__":
    unittest.main()
