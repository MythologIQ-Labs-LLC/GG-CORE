#!/usr/bin/env python3
"""GG-CORE #114 source/installed conformance claims, never release authorization.

No network calls or implicit execution. Evidence is caller-supplied and
independently unauthenticated: RECORDED_PASS is not verified acceptance.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

SHA40 = re.compile(r"[0-9a-f]{40}\Z")
SHA64 = re.compile(r"[0-9a-f]{64}\Z")
PLATFORMS = {"any", "linux", "windows", "macos"}
STATUSES = {"PASS", "FAIL", "NOT_RUN", "UNSUPPORTED"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_manifest(manifest):
    require(manifest.get("schema_version") == 1, "manifest schema_version must be 1")
    cases = manifest.get("cases")
    require(isinstance(cases, list) and cases, "nonempty cases required")
    seen = set()
    for row in cases:
        require(isinstance(row, dict), "invalid case")
        ident = row.get("id")
        require(isinstance(ident, str) and re.fullmatch(r"[a-z0-9_]+", ident), "invalid case ID")
        require(ident not in seen, "duplicate case ID: " + ident)
        seen.add(ident)
        require(row.get("tier") in ("source", "installed"), "invalid tier: " + ident)
        require(row.get("platform") in PLATFORMS, "invalid platform: " + ident)
        flags = row.get("features")
        require(isinstance(flags, list) and all(isinstance(f, str) and f for f in flags),
                "invalid features: " + ident)
        require(len(flags) == len(set(flags)), "duplicate feature: " + ident)
        require(type(row.get("model_required")) is bool, "model_required boolean: " + ident)
    return {row["id"]: row for row in cases}


def file_hash(path, claimed, label):
    require(isinstance(claimed, str) and SHA64.fullmatch(claimed) is not None,
            label + " SHA-256 missing/invalid")
    require(isinstance(path, str) and path, label + " path missing")
    file = Path(path)
    require(file.is_file(), label + " file missing")
    digest = hashlib.sha256()
    with file.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    require(digest.hexdigest() == claimed, label + " content hash mismatch")
    return claimed


def report(manifest, submitted, source_sha):
    require(isinstance(source_sha, str) and SHA40.fullmatch(source_sha) is not None,
            "exact 40-character source_sha required")
    cases = validate_manifest(manifest)
    require(isinstance(submitted, list), "evidence must be a list")
    entries = {}
    for item in submitted:
        require(isinstance(item, dict), "invalid evidence entry")
        ident = item.get("id")
        require(isinstance(ident, str) and ident in cases, "unknown case ID")
        require(ident not in entries, "duplicate evidence entry: " + ident)
        declaration = cases[ident]
        require(item.get("source_sha") == source_sha, "stale source SHA: " + ident)
        state = item.get("status")
        require(state in STATUSES, "invalid status: " + ident)
        platform = item.get("platform")
        require(platform in PLATFORMS - {"any"}, "actual platform required: " + ident)
        require(declaration["platform"] in ("any", platform), "wrong platform: " + ident)
        require(item.get("features") == declaration["features"], "feature mismatch: " + ident)
        if state == "PASS":
            require(isinstance(item.get("evidence_ref"), str) and item["evidence_ref"].strip(),
                    "PASS needs evidence_ref: " + ident)
            if declaration["tier"] == "installed":
                file_hash(item.get("binary_path"), item.get("binary_sha256"), "binary")
            if declaration["model_required"]:
                file_hash(item.get("model_path"), item.get("model_sha256"), "model")
        elif state == "UNSUPPORTED":
            require(isinstance(item.get("owner_decision_ref"), str)
                    and item["owner_decision_ref"].strip(),
                    "UNSUPPORTED requires owner disposition: " + ident)
        entries[ident] = item

    outcomes = []
    counts = {"RECORDED_PASS": 0, "FAIL": 0, "NOT_RUN": 0, "UNSUPPORTED": 0}
    for ident, definition in cases.items():
        item = entries.get(ident)
        status = "NOT_RUN" if item is None else (
            "RECORDED_PASS" if item["status"] == "PASS" else item["status"])
        counts[status] += 1
        outcomes.append({
            "id": ident, "tier": definition["tier"],
            "platform": definition["platform"], "features": definition["features"],
            "status": status,
            "evidence_ref": None if item is None else item.get("evidence_ref"),
        })
    return {
        "schema_version": 1,
        "source_sha": source_sha,
        "status": "HOLD",
        "release_authorized": False,
        "evidence_attested": False,
        "counts": counts,
        "cases": outcomes,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=Path(__file__).with_name("conformance_inventory.json"))
    parser.add_argument("--evidence", type=Path, help="JSON list of caller-provided evidence assertions")
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--fail-on-incomplete", action="store_true")
    args = parser.parse_args(argv)
    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        evidence = json.loads(args.evidence.read_text(encoding="utf-8")) if args.evidence else []
        output = report(manifest, evidence, args.source_sha)
    except (ValueError, OSError, json.JSONDecodeError) as exc:
        parser.error(str(exc))
    print(json.dumps(output, indent=2, sort_keys=True))
    if args.fail_on_incomplete and output["counts"]["RECORDED_PASS"] != len(output["cases"]):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
