#!/usr/bin/env python3
"""Fail-closed, offline Windows audit-wire and feature-matrix qualification (#213).

An executable evidence collector, NOT evidence of native acceptance until run.
Does not download dependencies/models, change git, or publish artifacts.
"""

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

# No __pycache__ in the checkout: it would itself be an ignored build-tree entry.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_gate_inputs as inputs  # noqa: E402

MODEL_RELATIVE = Path("models/qwen2.5-0.5b-instruct-q4_k_m.gguf")
# Must match the release workflow's pinned GGUF artifact. Accepting any
# caller-provided matching digest would let a different model masquerade as
# the approved native qualification fixture.
APPROVED_MODEL_SHA256 = "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db"
SHA40 = re.compile(r"[0-9a-f]{40}\Z")
SHA64 = re.compile(r"[0-9a-f]{64}\Z")
TEST_SUMMARY = re.compile(r"^test result: .*", re.MULTILINE)


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def checked_stdout(argv, cwd):
    return subprocess.check_output(argv, cwd=cwd, text=True, stderr=subprocess.STDOUT).strip()


def command_matrix():
    flags = ["--locked", "--offline"]
    return [
        ("metadata", ["cargo", "metadata", *flags, "--format-version", "1"]),
        ("fmt", ["cargo", "fmt", "--check"]),
        ("clippy_default", ["cargo", "clippy", *flags, "--all-targets", "--", "-D", "warnings"]),
        ("clippy_full", ["cargo", "clippy", *flags, "--all-targets", "--features", "onnx,gguf,ffi", "--", "-D", "warnings"]),
        ("audit_wire", ["cargo", "test", *flags, "--test", "audit_timestamp_compat_test"]),
        ("test_default", ["cargo", "test", *flags]),
        *[("test_" + feature, ["cargo", "test", *flags, "--features", feature])
          for feature in ("advanced", "ffi", "python", "onnx", "gguf")],
        ("test_full", ["cargo", "test", *flags, "--features", "onnx,gguf,ffi", "--no-fail-fast"]),
        # cargo-deny has its own advisory DB fetcher. CARGO_NET_OFFLINE
        # alone does not prohibit that network access; require offline mode
        # and the already-provisioned, freshness-checked advisory database.
        ("cargo_deny", ["cargo", "deny", "--offline", "--locked", "check"]),
        ("release_build", ["cargo", "build", *flags, "--release", "--features", "onnx,gguf,ffi"]),
    ]


def run_step(label, argv, cwd, env, evidence_dir, report):
    log = evidence_dir / (label + ".log")
    started = time.monotonic()
    with log.open("w", encoding="utf-8", errors="replace") as output:
        try:
            result = subprocess.run(argv, cwd=cwd, env=env, stdout=output,
                                    stderr=subprocess.STDOUT, check=False)
            code = result.returncode
        except OSError as error:
            output.write(f"Unable to launch {argv[0]}: {error}\n")
            code = 127
    text = log.read_text(encoding="utf-8", errors="replace")
    report["steps"].append({
        "name": label, "argv": argv, "exit_code": code,
        "elapsed_s": round(time.monotonic() - started, 3),
        "log": log.name, "test_summaries": TEST_SUMMARY.findall(text),
    })
    if code != 0:
        raise RuntimeError(f"{label} exit={code}; see {log}")


def validate_checkout(repo, expected_head, expected_model):
    if not SHA40.fullmatch(expected_head) or not SHA64.fullmatch(expected_model):
        raise ValueError("Source must be 40 hex and model SHA-256 must be 64 hex")
    if expected_model != APPROVED_MODEL_SHA256:
        raise RuntimeError("Provided model digest is not the approved release GGUF fixture")
    head = checked_stdout(["git", "rev-parse", "HEAD"], repo)
    if head != expected_head:
        raise RuntimeError(f"Checkout {head} differs from expected {expected_head}")
    # Include untracked AND ignored files: a local .cargo/config, a gitignored
    # source module or another sidecar can change the compiled artifact despite
    # an unchanged Git HEAD. Exclude only the independently hash-checked real
    # fixture, never a whole source directory.
    dirty = checked_stdout(["git", "status", "--porcelain", "--ignored", "--untracked-files=all",
                            "--", ".", ":!" + MODEL_RELATIVE.as_posix()], repo)
    ignored = [line[3:] for line in dirty.splitlines() if line.startswith("!! ")]
    if ignored:
        raise RuntimeError(f"Ignored build inputs present; native evidence requires a clean export: {ignored}")
    if dirty:
        raise RuntimeError("Uncommitted or untracked source exists; native evidence requires exact HEAD")
    model = repo / MODEL_RELATIVE
    if not model.is_file():
        raise RuntimeError("Pinned real GGUF fixture missing")
    with model.open("rb") as stream:
        if stream.read(4) != b"GGUF":
            raise RuntimeError("Pinned real GGUF fixture is not GGUF (possibly LFS pointer)")
    model_hash = digest(model)
    if model_hash != expected_model:
        raise RuntimeError(f"Real model hash mismatch: {model_hash}")
    return model_hash


def require_offline_windows():
    if platform.system() != "Windows":
        raise RuntimeError("Native acceptance requires Windows, not a source-only Linux runner")
    # A real air-gapped guest has no UP network adapters. No advisory fallback.
    script = "@(Get-NetAdapter -ErrorAction Stop | Where-Object { $_.Status -eq 'Up' }).Count"
    output = checked_stdout(["powershell.exe", "-NoProfile", "-NonInteractive",
                             "-Command", script], None)
    if output != "0":
        raise RuntimeError(f"Offline guest required: active network adapter count = {output!r}")


def collect_artifacts(core, target_dir, report):
    # The target is private to this single qualification attempt. Hashing
    # core-runtime/target could silently include stale binaries from previous
    # heads and falsely attribute them to this source revision.
    binaries = sorted((target_dir / "debug/deps").glob("*.exe"))
    if not binaries:
        raise RuntimeError("No compiled Windows test binaries were found")
    report["test_binaries"] = [
        {"path": str(p.relative_to(target_dir)), "sha256": digest(p)} for p in binaries
    ]
    release = target_dir / "release/gg-core-cli.exe"
    if not release.is_file():
        raise RuntimeError("No release-profile gg-core-cli.exe was produced")
    report["cli_release_sha256"] = digest(release)
    version = json.loads(checked_stdout([str(release), "version", "--json"], core))
    report["cli_version_json"] = version
    if version.get("commit") != report["source_sha"]:
        raise RuntimeError("Release CLI build commit does not match exact checkout")
    if set(version.get("features", [])) != {"onnx", "gguf", "ffi"}:
        raise RuntimeError("Release CLI feature provenance differs from tested build")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-head", required=True, help="Exact 40-char Git checkout commit")
    parser.add_argument("--model-sha256", required=True, help="Approved 64-char real GGUF SHA256")
    parser.add_argument("--evidence-dir", type=Path, required=True, help="Directory outside checkout")
    args = parser.parse_args(argv)
    repo = Path(__file__).resolve().parents[2]
    core = repo / "core-runtime"
    evidence = args.evidence_dir.resolve()
    report = {
        "status": "HOLD", "source_sha": args.expected_head,
        "started_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "host_os": platform.platform(), "steps": [],
    }
    receipt_allowed = False
    try:
        if evidence == repo or repo in evidence.parents:
            raise RuntimeError("Evidence directory must be outside the checkout")
        # Every acceptance attempt owns a fresh evidence tree and build
        # directory. Never overwrite an earlier run or reuse compiled bits.
        if evidence.is_symlink() or (evidence.exists() and any(evidence.iterdir())):
            raise RuntimeError("Evidence directory must be new or empty; previous receipts are immutable")
        evidence.mkdir(parents=True, exist_ok=True)
        receipt_allowed = True
        target_dir = evidence / "cargo-target"
        if target_dir.exists() or target_dir.is_symlink():
            raise RuntimeError("Qualification Cargo target directory must start absent")
        require_offline_windows()
        report["model_sha256"] = validate_checkout(repo, args.expected_head, args.model_sha256)
        report["build_inputs"] = inputs.require_controlled_build(repo, os.environ)
        report.update(inputs.toolchain_identity(core))
        report["cargo_deny_version"] = checked_stdout(["cargo", "deny", "--version"], core)
        env = dict(os.environ)
        env["CARGO_NET_OFFLINE"] = "true"
        env["GG_CORE_REQUIRE_REAL_MODEL"] = "1"
        # Override any inherited CARGO_TARGET_DIR. This rules out false
        # acceptance of cached test executables from a different Git head.
        env["CARGO_TARGET_DIR"] = str(target_dir)
        report["cargo_target_dir"] = str(target_dir)
        for label, command in command_matrix():
            run_step(label, command, core, env, evidence, report)
        collect_artifacts(core, target_dir, report)
        report["status"] = "SOURCE_AND_NATIVE_TEST_MATRIX_PASS__RELEASE_STILL_HOLD"
        return 0
    except Exception as error:
        report["error"] = str(error)
        print(f"NATIVE_ACCEPTANCE_HOLD: {error}", file=sys.stderr)
        return 1
    finally:
        report["finished_utc"] = dt.datetime.now(dt.timezone.utc).isoformat()
        if receipt_allowed:
            (evidence / "receipt.json").write_text(
                json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
