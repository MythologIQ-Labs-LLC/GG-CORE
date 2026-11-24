#!/usr/bin/env python3
"""#114: verify an operator-staged CLI artifact by actually executing version --json.

The binary and its compiled feature set are pinned, not inferred from a
repository commit or a prior CI run. This is local source/binary identity
evidence only, NOT an installed feature, security, model, or release GO.
Runs only an explicitly supplied staged binary; do not execute untrusted files.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from conformance_receipts import SHA40, SHA64, validate_manifest
from daemon_json_contract import reject_non_json_constant, unique_json_pairs

TARGET_PLATFORMS = {"linux": "linux", "win32": "windows", "darwin": "macos"}
TARGET_RE = re.compile(r"(linux|windows|macos)-[a-zA-Z0-9_]+\\Z")
MAX_VERSION_BYTES = 8192


class ProbeError(Exception):
    """Fail closed with a non-sensitive diagnostic."""


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def staged_binary(stage, name, claimed_digest):
    if not isinstance(name, str) or name not in {"gg-core-cli", "gg-core-cli.exe"}:
        raise ProbeError("invalid-cli-name")
    if not SHA64.fullmatch(claimed_digest or ""):
        raise ProbeError("invalid-binary-sha256")
    root = stage.resolve(strict=True)
    path = root / name
    if path.is_symlink() or not path.is_file():
        raise ProbeError("missing-or-symlinked-cli")
    if sha256_file(path) != claimed_digest:
        raise ProbeError("binary-sha256-mismatch")
    return path


def validate_version(doc, source_sha, features, platform):
    if not isinstance(doc, dict) or doc.get("name") != "GG-CORE":
        raise ProbeError("unexpected-cli-identity")
    if doc.get("commit") != source_sha:
        raise ProbeError("source-commit-mismatch")
    if not isinstance(doc.get("version"), str) or not doc["version"]:
        raise ProbeError("missing-semver")
    if doc.get("protocol_version") != "V1":
        raise ProbeError("unsupported-protocol-version")
    target = doc.get("target")
    if not isinstance(target, str) or not TARGET_RE.fullmatch(target):
        raise ProbeError("invalid-target")
    if not target.startswith(platform + "-"):
        raise ProbeError("target-platform-mismatch")
    actual = doc.get("features")
    if not isinstance(actual, list) or any(not isinstance(x, str) for x in actual):
        raise ProbeError("invalid-feature-list")
    if len(set(actual)) != len(actual) or sorted(actual) != sorted(features):
        raise ProbeError("compiled-feature-mismatch")
    return target


def probe(manifest, case_id, stage, name, claimed_digest, source_sha, runner=None):
    if not isinstance(source_sha, str) or not SHA40.fullmatch(source_sha):
        raise ProbeError("invalid-source-sha")
    cases = validate_manifest(manifest)
    case = cases.get(case_id)
    if not case or case["interface"] != "cli" or case["tier"] != "installed":
        raise ProbeError("unknown-installed-cli-case")
    if case["model_required"]:
        raise ProbeError("model-dependent-cli-case-not-supported")
    platform = TARGET_PLATFORMS.get(sys.platform)
    if platform is None or case["platform"] not in ("any", platform):
        raise ProbeError("unsupported-or-mismatched-platform")
    path = staged_binary(stage, name, claimed_digest)
    execute = runner or subprocess.run
    try:
        result = execute(
            [str(path), "version", "--json"],
            cwd=str(stage.resolve(strict=True)), timeout=10,
            capture_output=True, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ProbeError("cli-version-execution-failed") from exc
    if result.returncode != 0:
        raise ProbeError("cli-version-exit-nonzero")
    if len(result.stdout) > MAX_VERSION_BYTES:
        raise ProbeError("cli-version-output-too-large")
    try:
        doc = json.loads(result.stdout, object_pairs_hook=unique_json_pairs,
                         parse_constant=reject_non_json_constant)
    except (ValueError, UnicodeError) as exc:
        raise ProbeError("cli-version-invalid-json") from exc
    target = validate_version(doc, source_sha, case["features"], platform)
    # This observation is locally checked, but the resulting JSON is not a
    # cryptographically attested statement about independent installation.
    return {
        "id": case_id, "source_sha": source_sha, "status": "PASS",
        "platform": platform, "features": case["features"],
        "binary_path": str(path), "binary_sha256": claimed_digest,
        "evidence_ref": "local-cli-version-sha256:" +
                        hashlib.sha256(result.stdout).hexdigest(),
        "observed_target": target, "observed_version": doc["version"],
        "evidence_attested": False,
    }


def exclusive_receipt(path, assertions):
    # O_EXCL prevents a retried probe from overwriting a prior receipt.
    payload = json.dumps(assertions, indent=2, sort_keys=True) + "\n"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        stream.write(payload)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path,
                        default=Path(__file__).with_name("conformance_inventory.json"))
    parser.add_argument("--case-id", required=True)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--binary", default="gg-core-cli.exe" if os.name == "nt"
                        else "gg-core-cli")
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"),
                              object_pairs_hook=unique_json_pairs,
                              parse_constant=reject_non_json_constant)
        evidence = probe(manifest, args.case_id, args.stage, args.binary,
                         args.binary_sha256, args.source_sha)
        exclusive_receipt(args.receipt, [evidence])
    except (ProbeError, ValueError, OSError, json.JSONDecodeError) as exc:
        print("FAIL: " + (str(exc) if isinstance(exc, ProbeError)
                          else "invalid-input-or-receipt"), file=sys.stderr)
        return 1
    print("RECORDED_PASS: CLI version identity only; release HOLD")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
