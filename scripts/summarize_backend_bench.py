#!/usr/bin/env python3
"""Summarize GGUF/ONNX/Hologram receipts without assuming backend parity.

Each source-reported run remains separate. This tool makes no winner claim and
cannot independently attest an inference binary, hardware, model, or output.
"""
import argparse
import json
import sys
from pathlib import Path

from validate_backend_bench import (
    BACKENDS, TASKS, MAX_LINE_BYTES, MAX_RECEIPTS, MAX_TOTAL_BYTES,
    _reject_constant, _unique_object, bounded_lines, validate,
)


def read_receipts(path):
    """Read strict JSONL and reject ALL output if any receipt is invalid."""
    rows = []
    for number, raw in bounded_lines(path):
        if not raw.strip():
            continue
        if len(rows) >= MAX_RECEIPTS:
            raise ValueError("receipt count exceeds safe limit")
        try:
            row = json.loads(raw.decode("utf-8"),
                             object_pairs_hook=_unique_object,
                             parse_constant=_reject_constant)
        except (ValueError, UnicodeError, RecursionError) as exc:
            raise ValueError(f"line {number}: invalid JSON: {exc}") from exc
        try:
            errors = validate(row)
        except (OverflowError, TypeError, ValueError) as exc:
            raise ValueError(f"line {number}: invalid numeric receipt") from exc
        if errors:
            raise ValueError(f"line {number}: {'; '.join(errors)}")
        rows.append(row)
    if not rows:
        raise ValueError("no valid receipts supplied")
    return rows

def safe_median(samples):
    ordered = sorted(samples)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return ordered[middle - 1] / 2 + ordered[middle] / 2


def describe_receipt(row):
    """Whitelist only non-content provenance; never copy prompt/output data."""
    data = {"declared_status": row["status"], "source_commit": row["source_commit"],
            "host_id": row["host_id"], "os": row["os"]}
    for key in ("model_sha256", "fixture_sha256"):
        if key in row:
            data[key] = row[key]
    if row["status"] in ("verified", "conditional"):
        samples = row["latency_ms"]
        data["reported_latency"] = {"samples": len(samples),
                                    "min_ms": min(samples), "median_ms": safe_median(samples),
                                    "max_ms": max(samples)}
        if row["task"] == "generation":
            data["reported_decode_tokens_per_second"] = row["decode_tokens_per_second"]
    else:
        data["reason_recorded"] = True
    return data


def summarize(rows):
    """Produce a full capability grid, with unpooled individual runs."""
    matrix = []
    for backend in sorted(BACKENDS):
        for task in sorted(TASKS):
            matches = [r for r in rows if r["backend"] == backend and r["task"] == task]
            matrix.append({"backend": backend, "task": task, "receipt_count": len(matches),
                           "reported_runs": [describe_receipt(r) for r in matches]})
    return {"schema_version": 1, "kind": "backend_benchmark_matrix",
            "evidence_level": "self-reported JSONL, not independently attested",
            "cross_backend_ranking": "prohibited: equivalence not established",
            "note": "Each run is separate; matching names or hashes do not prove parity.",
            "capabilities": matrix}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("jsonl", type=Path)
    args = parser.parse_args(argv)
    try:
        matrix = summarize(read_receipts(args.jsonl))
        print(json.dumps(matrix, indent=2, sort_keys=True, allow_nan=False))
    except (OSError, UnicodeError, ValueError, OverflowError) as exc:
        print(f"FAIL: no summary produced: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
