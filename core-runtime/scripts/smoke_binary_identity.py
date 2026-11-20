#!/usr/bin/env python3
"""CI smoke: execute the built GG-CORE binary and verify compiled provenance.

The hash is captured from this build, not compared with a release-approved
artifact. This only proves that the preflight can verify a real CLI build.
"""
import json
import os
import pathlib
import sys

from check_binary_identity import check, file_sha256
from cli_contract import check_public_cli


def main() -> int:
    commit = os.environ.get("GITHUB_SHA", "")
    name = "gg-core-cli.exe" if sys.platform == "win32" else "gg-core-cli"
    binary = pathlib.Path("target") / "debug" / name
    try:
        if not binary.is_file():
            raise ValueError("built GG-CORE CLI is missing")
        receipt = check(binary, commit, file_sha256(binary), set())
        receipt["public_cli"] = check_public_cli(binary, receipt["version"])
    except (ValueError, OSError) as exc:
        print(json.dumps({"result": "FAIL", "reason": str(exc)}))
        return 1
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
