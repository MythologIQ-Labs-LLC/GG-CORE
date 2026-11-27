"""Synthetic controls only: never establish actual installed model evidence."""
import copy
import hashlib
import json
import unittest

from pinned_latency_receipt import compare


class PinnedLatencyReceipt(unittest.TestCase):
    def setUp(self):
        self.doc = {
            "schema_version": 2, "producer_sha": "a" * 40,
            "model_sha256": "b" * 64, "daemon_sha256": "c" * 64,
            "prompt_sha256": "d" * 64, "model_id": "fixture",
            "host": {"platform": "test", "machine": "x86_64", "python": "3.12",
                     "cpu_count": 8},
            "requested": {"auth": True, "stream": False, "max_tokens": 64,
                          "timeout_ms": 120000, "waves": 20, "concurrency": [1, 2]},
            "packaged_binary_version": {"version": "0.1", "commit": "a" * 40,
                                        "protocol_version": "V1"},
            "results": [{"concurrency": n, "stats": {"duration_ms": {"p95": 0}},
                         "samples": [{"duration_ms": i + 1} for i in range(20 * n)]}
                        for n in (1, 2)],
        }
        self.pin = {k: copy.deepcopy(self.doc[k]) for k in (
            "producer_sha", "model_sha256", "daemon_sha256", "prompt_sha256",
            "model_id", "host", "requested")}
        self.pin["concurrency"] = 1

    def raw(self, repin=True):
        raw = json.dumps(self.doc).encode()
        if repin:
            self.pin["receipt_sha256"] = hashlib.sha256(raw).hexdigest()
        return raw

    def test_pin_match_derives_raw_p95_and_grants_no_authority(self):
        result = compare(self.raw(), self.pin)
        self.assertEqual(result["status"], "PIN_MATCH_ONLY")
        self.assertAlmostEqual(result["reported_resident_p95_ms"], 19.05)
        self.assertEqual(result["sample_count"], 20)
        self.assertFalse(result["execution_authorized"])
        self.assertFalse(result["evidence_attested"])

    def test_mutated_bytes_refused_even_with_matching_metadata(self):
        self.raw()
        self.doc["results"][0]["samples"][0]["duration_ms"] = 2
        with self.assertRaisesRegex(ValueError, "receipt-digest-mismatch"):
            compare(self.raw(False), self.pin)

    def test_rehashed_wrong_identity_refused(self):
        self.doc["daemon_sha256"] = "e" * 64
        with self.assertRaisesRegex(ValueError, "pinned-metadata-mismatch"):
            compare(self.raw(), self.pin)

    def test_wrong_host_or_request_refused(self):
        for field, key, value in (("host", "cpu_count", 4),
                                  ("requested", "max_tokens", 128),
                                  ("requested", "stream", 0)):
            with self.subTest(key=key):
                original = self.doc[field][key]
                self.doc[field][key] = value
                with self.assertRaisesRegex(ValueError, "pinned-metadata-mismatch"):
                    compare(self.raw(), self.pin)
                self.doc[field][key] = original

    def test_malformed_unselected_cohort_refused(self):
        self.doc["results"][1]["samples"].pop()
        with self.assertRaisesRegex(ValueError, "incomplete-samples"):
            compare(self.raw(), self.pin)

    def test_duplicate_cohort_refused(self):
        self.doc["results"][1] = self.doc["results"][0]
        with self.assertRaisesRegex(ValueError, "invalid-cohort"):
            compare(self.raw(), self.pin)

    def test_invalid_duration_refused(self):
        for value in (False, 0, -1, None, "1"):
            with self.subTest(value=value):
                self.doc["results"][0]["samples"][0]["duration_ms"] = value
                with self.assertRaisesRegex(ValueError, "invalid-duration"):
                    compare(self.raw(), self.pin)

    def test_insufficient_samples_refused(self):
        self.pin["requested"]["waves"] = self.doc["requested"]["waves"] = 19
        for row in self.doc["results"]:
            row["samples"] = row["samples"][:19 * row["concurrency"]]
        with self.assertRaisesRegex(ValueError, "insufficient-p95-samples"):
            compare(self.raw(), self.pin)

    def test_duplicate_json_key_refused_even_when_pinned(self):
        raw = self.raw().replace(b'"schema_version": 2', b'"schema_version": 2, "schema_version": 2')
        self.pin["receipt_sha256"] = hashlib.sha256(raw).hexdigest()
        with self.assertRaises(ValueError):
            compare(raw, self.pin)


if __name__ == "__main__":
    unittest.main()
