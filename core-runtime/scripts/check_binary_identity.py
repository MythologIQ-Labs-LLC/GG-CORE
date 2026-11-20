#!/usr/bin/env python3
"""Offline exact-executable identity preflight for GG-CORE (#114).

PASS establishes pinned binary/source identity only, NOT model, OS, security,
hardware, cancellation, or installed-runtime qualification.
"""
import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys

FEATURES = {"gguf", "onnx", "ffi", "python", "advanced", "cuda", "metal"}


def file_sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_version(binary: pathlib.Path) -> dict:
    try:
        proc = subprocess.run([str(binary.absolute()), "version", "--json"],
                              capture_output=True, text=True, timeout=15, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ValueError("version invocation failed") from exc
    if proc.returncode != 0:
        raise ValueError("version command failed")
    try:
        doc = json.loads(proc.stdout)
    except (json.JSONDecodeError, TypeError) as exc:
        raise ValueError("invalid version JSON") from exc
    if not isinstance(doc, dict):
        raise ValueError("invalid version JSON object")
    return doc


def check(binary: pathlib.Path, source_commit: str, binary_sha256: str,
          required: set[str]) -> dict:
    if not re.fullmatch(r"[0-9a-fA-F]{40}", source_commit):
        raise ValueError("source commit must be an exact 40-digit Git SHA")
    if not re.fullmatch(r"[0-9a-f]{64}", binary_sha256):
        raise ValueError("binary SHA-256 must be 64 lowercase hex digits")
    if not required <= FEATURES:
        raise ValueError("unknown required feature")
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("binary must be a regular file, not a symlink")
    before = file_sha256(binary)
    if before != binary_sha256:
        raise ValueError("binary SHA-256 mismatch")
    doc = read_version(binary)
    if file_sha256(binary) != before:
        raise ValueError("binary changed during identity check")
    if doc.get("name") != "GG-CORE":
        raise ValueError("invalid product identity")
    commit = doc.get("commit")
    if not isinstance(commit, str) or commit.lower() != source_commit.lower():
        raise ValueError("compiled source SHA mismatch")
    features = doc.get("features")
    if (not isinstance(features, list) or
            not all(isinstance(f, str) for f in features) or
            len(features) != len(set(features)) or
            not set(features) <= FEATURES):
        raise ValueError("malformed compiled feature list")
    if not required <= set(features):
        raise ValueError("missing compiled features")
    if doc.get("protocol_version") != "V1":
        raise ValueError("unexpected IPC protocol version")
    if not isinstance(doc.get("version"), str) or not doc["version"]:
        raise ValueError("missing runtime version")
    if not isinstance(doc.get("target"), str) or "-" not in doc["target"]:
        raise ValueError("invalid build target")
    return {"result": "PASS", "scope": "binary_identity_only",
            "binary_sha256": before, "source_commit": source_commit.lower(),
            "version": doc["version"], "target": doc["target"],
            "compiled_features": sorted(features), "required_features": sorted(required),
            "native_inference_qualified": False, "security_qualified": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--require-feature", action="append", default=[])
    args = parser.parse_args()
    try:
        receipt = check(args.binary, args.source_commit, args.binary_sha256,
                        set(args.require_feature))
    except (ValueError, OSError) as exc:
        print(json.dumps({"result": "FAIL", "scope": "binary_identity_only",
                          "reason": str(exc)}))
        return 1
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
