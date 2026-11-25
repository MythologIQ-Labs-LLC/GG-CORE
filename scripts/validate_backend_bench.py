#!/usr/bin/env python3
"""Validate provenance-rich backend benchmark JSONL receipts.

This is a schema/evidence gate, not an inference benchmark or backend ranking.
"""
import argparse
import json
import math
import re
import sys
from pathlib import Path

SHA256 = re.compile(r"^[a-fA-F0-9]{64}$")
STATUSES = {"verified", "conditional", "unsupported", "blocked", "unmeasured"}
BACKENDS = {"gguf", "onnx", "hologram"}
TASKS = {"generation", "embedding", "classification"}


def validate(row):
    """Return all errors for a single receipt without modifying it."""
    errors = []
    if not isinstance(row, dict):
        return ["receipt must be an object"]
    for key in ("schema_version", "backend", "task", "status", "source_commit",
                "host_id", "os", "model_sha256", "fixture_sha256"):
        if key not in row:
            errors.append(f"missing {key}")
    if row.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if row.get("backend") not in BACKENDS:
        errors.append("unknown backend")
    if row.get("task") not in TASKS:
        errors.append("unknown task")
    if row.get("status") not in STATUSES:
        errors.append("unknown status")
    for key in ("source_commit", "host_id", "os"):
        if not isinstance(row.get(key), str) or not row[key].strip():
            errors.append(f"{key} must be nonempty string")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", str(row.get("source_commit", ""))):
        errors.append("source_commit must be full 40-character SHA")
    for key in ("model_sha256", "fixture_sha256"):
        if not isinstance(row.get(key), str) or not SHA256.fullmatch(row[key]):
            errors.append(f"{key} must be SHA-256 hex")
    measured = row.get("status") in {"verified", "conditional"}
    samples = row.get("latency_ms")
    if measured:
        if not isinstance(samples, list) or not samples:
            errors.append("measured status requires raw latency_ms samples")
        elif any(type(s) not in (float, int) or not math.isfinite(s) or s < 0 for s in samples):
            errors.append("latency_ms contains invalid sample")
        if row.get("correctness_passed") is not True:
            errors.append("measured status requires correctness_passed=true")
        if row.get("task") == "generation" and not isinstance(row.get("decode_tokens_per_second"), (int, float)):
            errors.append("measured generation requires decode_tokens_per_second")
        if row.get("task") != "generation" and "decode_tokens_per_second" in row:
            errors.append("tokens/sec is not a valid non-generation throughput metric")
    else:
        if samples is not None or "decode_tokens_per_second" in row:
            errors.append("unmeasured/blocked/unsupported must not publish throughput or samples")
        if not isinstance(row.get("reason"), str) or not row["reason"].strip():
            errors.append("non-measured status requires reason")
    if "decode_tokens_per_second" in row:
        n = row["decode_tokens_per_second"]
        if type(n) not in (float, int) or not math.isfinite(n) or n < 0:
            errors.append("decode_tokens_per_second must be finite nonnegative number")
    if any(k in row for k in ("p50_ms", "p95_ms", "p99_ms")):
        errors.append("precomputed percentile claims are prohibited; derive summaries from raw samples")
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("jsonl", type=Path)
    args = parser.parse_args(argv)
    failed = count = 0
    with args.jsonl.open(encoding="utf-8") as stream:
        for line_no, line in enumerate(stream, 1):
            if not line.strip():
                continue
            count += 1
            try:
                row = json.loads(line, parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))
                errors = validate(row)
            except ValueError as exc:
                errors = [f"invalid JSON: {exc}"]
            for err in errors:
                failed += 1
                print(f"{args.jsonl}:{line_no}: {err}", file=sys.stderr)
    if not count:
        print("empty JSONL receipts", file=sys.stderr)
        return 1
    if failed:
        return 1
    print(f"validated {count} backend benchmark receipts")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
