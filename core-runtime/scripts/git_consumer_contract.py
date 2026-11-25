#!/usr/bin/env python3
"""Cross-platform external Cargo Git consumer identity contract (#113)."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SHA = re.compile(r"^[0-9a-f]{40}$")


def run(args, cwd, env, timeout=300):
    result = subprocess.run(args, cwd=cwd, env=env, text=True,
                            capture_output=True, timeout=timeout, check=False)
    if result.returncode:
        raise RuntimeError(f"{args[0]} {args[1]} exited {result.returncode}: "
                           f"{result.stderr[-1200:]}")
    return result.stdout


def main():
    sha = run(["git", "rev-parse", "HEAD"], ROOT, os.environ).strip().lower()
    if not SHA.fullmatch(sha):
        raise RuntimeError("producer source checkout has no exact Git SHA")
    env = dict(os.environ)
    # Deliberately disagree with the Git checkout: a consuming app has its
    # own CI commit; it is not the producer's build provenance.
    env["GITHUB_SHA"] = ("f" if sha != "f" * 40 else "e") * 40
    env.pop("GG_CORE_BUILD_COMMIT", None)

    with tempfile.TemporaryDirectory(prefix="gg-core-git-consumer-") as temp:
        root = Path(temp)
        (root / "src").mkdir()
        url = ROOT.as_uri()
        (root / "Cargo.toml").write_text(
            '[package]\nname = "gg-core-git-consumer-test"\n'
            'version = "0.0.1"\nedition = "2021"\n'
            '[dependencies]\n'
            f'gg-core = {{ git = "{url}", rev = "{sha}", '
            'version = "=0.10.0-rc.1", default-features = false }\n',
            encoding="utf-8")
        (root / "src" / "main.rs").write_text(
            'fn main() {\n'
            '    let _ = gg_core::models::backend_capabilities::'
            'compiled_backend_capabilities();\n'
            '    println!("{}|{}", gg_core::build_identity::VERSION, '
            'gg_core::build_identity::COMMIT);\n'
            '}\n', encoding="utf-8")

        run(["cargo", "generate-lockfile"], root, env, timeout=450)
        lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
        matches = [pkg for pkg in lock["package"] if pkg["name"] == "gg-core"]
        if len(matches) != 1:
            raise RuntimeError("external lockfile has zero/multiple GG-CORE crates")
        package = matches[0]
        if (package["version"] != "0.10.0-rc.1" or
                not package.get("source", "").endswith("#" + sha)):
            raise RuntimeError("external Cargo.lock lost exact version/source SHA")
        metadata = json.loads(run(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            root, env, timeout=450))
        matched = [pkg for pkg in metadata["packages"]
                   if pkg["name"] == "gg-core" and pkg["version"] == "0.10.0-rc.1"]
        if len(matched) != 1 or not matched[0].get("source", "").endswith("#" + sha):
            raise RuntimeError("external Cargo metadata resolved wrong producer")
        receipt = {"version": "0.10.0-rc.1", "source_sha": sha,
                   "metadata": "PASS", "execution": "NOT_RUN"}
        if sys.platform.startswith("linux"):
            result = run(["cargo", "run", "--locked", "--quiet"],
                         root, env, timeout=900).strip()
            if result != f"0.10.0-rc.1|{sha}":
                raise RuntimeError("external binary embedded wrong producer revision")
            receipt["execution"] = "PASS"
        print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"FAIL exact Git consumer: {error}", file=sys.stderr)
        sys.exit(1)
