#!/usr/bin/env python3
"""Validate task-aware backend benchmark evidence, not inference performance."""
import argparse
import json
import math
import re
import sys
from pathlib import Path

MAX_RECEIPTS = 2000
MAX_LINE_BYTES = 2 * 1024 * 1024
MAX_TOTAL_BYTES = 32 * 1024 * 1024

SHA256 = re.compile(r"[0-9a-fA-F]{64}\Z")
SHA40 = re.compile(r"[0-9a-fA-F]{40}\Z")
STATUSES = {"verified", "conditional", "unsupported", "blocked", "unmeasured"}
BACKENDS = {"gguf", "onnx", "hologram"}
TASKS = {"generation", "embedding", "classification"}


def finite_nonnegative(value):
    if type(value) not in (int, float) or value < 0:
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def validate(row):
    """Return diagnostic messages; do not mutate supplied benchmark receipts."""
    if not isinstance(row, dict):
        return ["receipt must be an object"]
    errors = []
    for key, allowed in (("backend", BACKENDS), ("task", TASKS), ("status", STATUSES)):
        value = row.get(key)
        if not isinstance(value, str) or value not in allowed:
            errors.append(f"invalid {key}")
    if type(row.get("schema_version")) is not int or row["schema_version"] != 1:
        errors.append("schema_version must be integer 1")
    if not isinstance(row.get("source_commit"), str) or not SHA40.fullmatch(row["source_commit"]):
        errors.append("source_commit must be a full 40-character Git SHA")
    for key in ("host_id", "os"):
        if not isinstance(row.get(key), str) or not row[key].strip():
            errors.append(f"{key} must be a nonempty string")
    measured = isinstance(row.get("status"), str) and row["status"] in {"verified", "conditional"}
    for key in ("model_sha256", "fixture_sha256"):
        value = row.get(key)
        if measured or value is not None:
            if not isinstance(value, str) or not SHA256.fullmatch(value):
                errors.append(f"{key} must be full SHA-256 hex when provided or measured")
    if measured:
        samples = row.get("latency_ms")
        if (not isinstance(samples, list) or not samples or
                any(not finite_nonnegative(x) for x in samples)):
            errors.append("measured latency_ms must contain finite nonnegative raw samples")
        if row.get("correctness_passed") is not True:
            errors.append("measured status requires correctness_passed=true")
        if row.get("task") == "generation" and "decode_tokens_per_second" not in row:
            errors.append("measured generation requires decode_tokens_per_second")
        if row.get("task") != "generation" and "decode_tokens_per_second" in row:
            errors.append("non-generation tasks cannot claim decode tokens/sec")
    else:
        if any(key in row for key in ("latency_ms", "decode_tokens_per_second", "correctness_passed")):
            errors.append("unmeasured statuses must not claim samples, throughput or correctness")
        if not isinstance(row.get("reason"), str) or not row["reason"].strip():
            errors.append("unmeasured statuses require a reason")
    if "decode_tokens_per_second" in row:
        value = row["decode_tokens_per_second"]
        if not finite_nonnegative(value):
            errors.append("decode_tokens_per_second must be finite nonnegative number")
    if any(key in row for key in ("p50_ms", "p95_ms", "p99_ms")):
        errors.append("precomputed percentile claims prohibited; derive from raw samples")
    return errors


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def _reject_constant(value):
    raise ValueError(f"non-JSON numeric constant {value}")


def bounded_lines(path):
    total_bytes = 0
    with path.open("rb") as stream:
        for number, raw in enumerate(iter(lambda: stream.readline(MAX_LINE_BYTES + 1), b""), 1):
            total_bytes += len(raw)
            if len(raw) > MAX_LINE_BYTES:
                raise ValueError(f"line {number}: receipt size ceiling exceeded")
            if total_bytes > MAX_TOTAL_BYTES:
                raise ValueError(f"line {number}: evidence total size ceiling exceeded")
            yield number, raw


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("jsonl", type=Path)
    args = parser.parse_args(argv)
    failed = count = 0
    try:
        for line_no, line in bounded_lines(args.jsonl):
            if not line.strip():
                continue
            count += 1
            if count > MAX_RECEIPTS:
                raise ValueError("receipt count exceeds safe limit")
            try:
                row = json.loads(line, object_pairs_hook=_unique_object,
                                 parse_constant=_reject_constant)
                errors = validate(row)
            except (ValueError, TypeError, UnicodeError, OverflowError, RecursionError) as exc:
                errors = [f"invalid JSON or receipt: {exc}"]
            for error in errors:
                failed += 1
                print(f"{args.jsonl}:{line_no}: {error}", file=sys.stderr)
    except (OSError, UnicodeError, ValueError) as exc:
        print(f"receipt input rejected: {exc}", file=sys.stderr)
        return 1
    if not count:
        print("empty JSONL receipts", file=sys.stderr)
        return 1
    if failed:
        return 1
    print(f"validated {count} backend benchmark receipts")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
