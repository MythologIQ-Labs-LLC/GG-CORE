#!/usr/bin/env python3
"""Offline comparison of independently pinned packaged IPC latency receipts.

A match checks bytes and recorded metadata, NOT actual daemon/model identity,
hardware, security, quality, or execution authority. Trust the pin separately.
"""
import argparse
import hashlib
import json
import sys
from pathlib import Path

from bench_packaged_ipc import quantile
from validate_backend_bench import (
    SHA40, SHA256, MAX_LINE_BYTES, _unique_object, _reject_constant,
    finite_nonnegative,
)


def read_bounded(path):
    with path.open("rb") as stream:
        raw = stream.read(MAX_LINE_BYTES + 1)
    if len(raw) > MAX_LINE_BYTES:
        raise ValueError("input-too-large")
    return raw


def strict_object(raw):
    value = json.loads(raw, object_pairs_hook=_unique_object,
                       parse_constant=_reject_constant)
    if not isinstance(value, dict):
        raise ValueError("invalid-object")
    return value


def check_pin(pin):
    fields = {"receipt_sha256", "producer_sha", "model_sha256", "daemon_sha256",
              "prompt_sha256", "model_id", "host", "requested", "concurrency"}
    if not isinstance(pin, dict) or set(pin) != fields:
        raise ValueError("invalid-pin-fields")
    for field in ("receipt_sha256", "model_sha256", "daemon_sha256", "prompt_sha256",
                  "producer_sha"):
        pattern = SHA40 if field == "producer_sha" else SHA256
        if not isinstance(pin[field], str) or not pattern.fullmatch(pin[field]):
            raise ValueError("invalid-pin-digest")
    if not isinstance(pin["model_id"], str) or not pin["model_id"].strip():
        raise ValueError("invalid-model-id")
    host = pin["host"]
    if not isinstance(host, dict) or not all(
        isinstance(host.get(k), str) and host[k].strip()
        for k in ("platform", "machine", "python")
    ) or not positive_int(host.get("cpu_count")):
        raise ValueError("invalid-host")
    request = pin["requested"]
    if not isinstance(request, dict) or request.get("auth") is not True:
        raise ValueError("invalid-request")
    if type(request.get("stream")) is not bool or not all(
        positive_int(request.get(k)) for k in ("max_tokens", "timeout_ms", "waves")
    ):
        raise ValueError("invalid-request")
    cohorts = request.get("concurrency")
    if not isinstance(cohorts, list) or not cohorts or any(
        not positive_int(n) or n > 64 for n in cohorts
    ) or len(set(cohorts)) != len(cohorts):
        raise ValueError("invalid-concurrency")
    if not positive_int(pin["concurrency"]) or pin["concurrency"] not in cohorts:
        raise ValueError("invalid-selected-cohort")


def positive_int(value):
    return type(value) is int and value > 0


def match_metadata(doc, pin):
    if type(doc.get("schema_version")) is not int or doc["schema_version"] != 2:
        raise ValueError("invalid-receipt-schema")
    for field in ("producer_sha", "model_sha256", "daemon_sha256", "prompt_sha256",
                  "model_id", "host", "requested"):
        # JSON encoding retains distinctions such as boolean versus integer.
        if json.dumps(doc.get(field), sort_keys=True) != json.dumps(pin[field], sort_keys=True):
            raise ValueError("pinned-metadata-mismatch")
    version = doc.get("packaged_binary_version")
    if not isinstance(version, dict) or version.get("commit") != pin["producer_sha"]:
        raise ValueError("binary-source-mismatch")
    if (version.get("protocol_version") != "V1"
            or not isinstance(version.get("version"), str) or not version["version"].strip()):
        raise ValueError("invalid-binary-version")


def cohort_samples(doc, pin):
    results = doc.get("results")
    expected = pin["requested"]["concurrency"]
    if not isinstance(results, list) or len(results) != len(expected):
        raise ValueError("invalid-cohorts")
    seen = set()
    selected = None
    for row in results:
        if not isinstance(row, dict):
            raise ValueError("invalid-cohort")
        count = row.get("concurrency")
        if not positive_int(count) or count not in expected or count in seen:
            raise ValueError("invalid-cohort")
        seen.add(count)
        samples = row.get("samples")
        if not isinstance(samples, list) or len(samples) != count * pin["requested"]["waves"]:
            raise ValueError("incomplete-samples")
        durations = []
        for sample in samples:
            value = sample.get("duration_ms") if isinstance(sample, dict) else None
            if not finite_nonnegative(value) or value == 0:
                raise ValueError("invalid-duration")
            durations.append(value)
        if count == pin["concurrency"]:
            selected = durations
    if selected is None or len(selected) < 20:
        raise ValueError("insufficient-p95-samples")
    return selected


def compare(raw, pin):
    check_pin(pin)
    digest = hashlib.sha256(raw).hexdigest()
    if digest != pin["receipt_sha256"].lower():
        raise ValueError("receipt-digest-mismatch")
    doc = strict_object(raw)
    match_metadata(doc, pin)
    durations = cohort_samples(doc, pin)
    return {"status": "PIN_MATCH_ONLY", "receipt_sha256": digest,
            "producer_sha": pin["producer_sha"], "model_sha256": pin["model_sha256"],
            "daemon_sha256": pin["daemon_sha256"], "concurrency": pin["concurrency"],
            "sample_count": len(durations), "reported_request_p95_ms": quantile(durations, .95),
            "evidence_attested": False, "execution_authorized": False,
            "note": "Descriptive sample percentile; not SLA, quality or installed qualification."}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--approved-pin", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        report = compare(read_bounded(args.receipt), strict_object(read_bounded(args.approved_pin)))
        print(json.dumps(report, sort_keys=True, allow_nan=False))
    except (ValueError, OSError, UnicodeError, RecursionError, OverflowError):
        print("FAIL: pinned latency receipt rejected", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
