#!/usr/bin/env python3
"""Fail-closed native Windows CUDA hidden-device qualification gate.

Runs only the pinned direct-backend no-device refusal control; it does NOT
qualify authenticated daemon IPC, the P0 scheduler, GPU execution, or v1.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

MODEL = Path("models/qwen2.5-0.5b-instruct-q4_k_m.gguf")
APPROVED_SHA256 = "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db"
TEST_PASS = re.compile(r"test result: ok\.\s+2 passed;\s+0 failed;\s+0 ignored;\s+0 measured;\s+0 filtered out;")
EXPECTED_TESTS = (
    "test cuda_build_without_a_visible_device_refuses_offload_but_serves_cpu ... ok",
    "test common::fixture_integrity_tests::pinned_fixture_check_rejects_wrong_bytes_and_wrong_hash ... ok",
)
PASS_MARKER = "GPUQUAL hidden_device.qual_status=PASS"


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def exact_test_binary(build_log, target, binary):
    """Select the single executed artifact from Cargo's machine-readable build."""
    deps = (target / "debug" / "deps").resolve()
    selected = []
    for line in build_log.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict) or event.get("reason") != "compiler-artifact":
            continue
        spec = event.get("target") or {}
        if spec.get("name") != binary or "test" not in spec.get("kind", []):
            continue
        raw = event.get("executable")
        if not isinstance(raw, str):
            continue
        candidate = Path(raw)
        if (not candidate.is_absolute() or candidate.is_symlink()
                or candidate.suffix.lower() != ".exe"
                or candidate.resolve().parent != deps or not candidate.is_file()):
            raise RuntimeError(f"{binary}: Cargo reported an unsafe test executable path")
        selected.append(candidate.resolve())
    if len(selected) != 1:
        raise RuntimeError(
            f"{binary}: expected exactly one Cargo-built test executable, got {len(selected)}"
        )
    return selected[0]



def checked(argv, cwd=None):
    return subprocess.check_output(argv, cwd=cwd, text=True, stderr=subprocess.STDOUT).strip()


# Build-sensitive locations that must not contain Git-ignored additions.
# `git status --porcelain` hides ignored files, including source files.
BUILD_INPUT_PATHS = (
    "core-runtime/src",
    "core-runtime/tests",
    "core-runtime/benches",
    "core-runtime/build.rs",
    "core-runtime/.cargo",
    ".cargo",
)

# Reject build-affecting Cargo/Rust overrides rather than inheriting them into
# a supposedly exact-source offline compilation. Credentials and cache paths
# must not be included in the public receipt.
BUILD_OVERRIDE_NAMES = frozenset({
    "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER", "RUSTDOCFLAGS", "CARGO_BUILD_TARGET",
    "CARGO_HOME", "CARGO_TARGET_DIR", "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTC_WRAPPER",
})


def validate_build_inputs(repo, environ):
    ignored = checked([
        "git", "ls-files", "--others", "--ignored", "--exclude-standard",
        "--", *BUILD_INPUT_PATHS,
    ], repo)
    if ignored:
        raise RuntimeError("Ignored build input exists under source/Cargo paths")
    overrides = sorted(
        name for name in environ
        if name.upper() in BUILD_OVERRIDE_NAMES
        or name.upper().startswith(("CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_BUILD_"))
    )
    if overrides:
        raise RuntimeError("Unapproved compilation environment override(s): "
                           + ", ".join(overrides))


def validate(repo, expected):
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("Expected Git revision must be the full 40-character SHA")
    if checked(["git", "rev-parse", "HEAD"], repo) != expected:
        raise RuntimeError("Checkout differs from pinned expected head")
    dirty = checked(
        ["git", "status", "--porcelain", "--untracked-files=all",
         "--", ".", ":!" + MODEL.as_posix()], repo
    )
    if dirty:
        raise RuntimeError("Dirty or untracked source; exact-head evidence refused")
    validate_build_inputs(repo, os.environ)
    model = repo / MODEL
    if not model.is_file():
        raise RuntimeError("Approved model file is missing")
    with model.open("rb") as source:
        if source.read(4) != b"GGUF":
            raise RuntimeError("Model is not GGUF (possibly an LFS pointer)")
    if digest(model) != APPROVED_SHA256:
        raise RuntimeError("Unapproved model digest")


def require_offline_windows():
    if platform.system() != "Windows":
        raise RuntimeError("Native Windows GPU acceptance cannot run on this OS")
    count = checked([
        "powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
        "@(Get-NetAdapter -ErrorAction Stop | Where-Object { $_.Status -eq 'Up' }).Count"
    ])
    if count != "0":
        raise RuntimeError("Guest must be air-gapped; active network adapters: " + count)


def evidence_pass(log, exit_code):
    return (exit_code == 0 and PASS_MARKER in log
            and TEST_PASS.search(log) is not None
            and all(name in log.splitlines() for name in EXPECTED_TESTS))


def run(repo, evidence, report):
    validate(repo, report["source_sha"])
    require_offline_windows()
    target = evidence / "cargo-target"
    target.mkdir()
    env = dict(os.environ)
    env.update({
        "CUDA_VISIBLE_DEVICES": "-1",
        "GG_EXPECT_NO_CUDA_DEVICE": "1",
        "GG_CORE_REQUIRE_REAL_MODEL": "1",
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TARGET_DIR": str(target.resolve()),
    })
    binary = "gguf_cuda_hidden_device_test"
    build_cmd = ["cargo", "test", "--locked", "--offline", "--features",
                 "gguf-cuda", "--test", binary, "--no-run", "--message-format=json"]
    build_log = evidence / "native-build.log"
    report["build_command"] = build_cmd
    report["environment_controls"] = {
        "CUDA_VISIBLE_DEVICES": "-1",
        "GG_EXPECT_NO_CUDA_DEVICE": "1",
        "CARGO_NET_OFFLINE": "true",
    }
    started = time.monotonic()
    try:
        built = subprocess.run(
            build_cmd, cwd=repo / "core-runtime", env=env, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, text=True, errors="replace",
            timeout=3600, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeError(f"Native test compilation failed: {error}") from error
    build_log.write_text(built.stdout, encoding="utf-8")
    if built.returncode:
        raise RuntimeError("Native test compilation failed; inspect native-build.log")
    executable = exact_test_binary(built.stdout, target, binary)
    executable_hash = digest(executable)
    cmd = [str(executable), "--nocapture", "--test-threads=1"]
    report["command"] = cmd
    report["build_log"] = build_log.name
    with (evidence / "native-test.log").open("w", encoding="utf-8") as logfile:
        try:
            completed = subprocess.run(
                cmd, cwd=repo / "core-runtime", env=env,
                stdout=logfile, stderr=subprocess.STDOUT,
                check=False, timeout=3600,
            )
            code = completed.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            logfile.write(f"Command failed to complete: {error}\n")
            code = 127
    report["elapsed_seconds"] = round(time.monotonic() - started, 3)
    report["exit_code"] = code
    if digest(executable) != executable_hash:
        raise RuntimeError("Native test executable changed after execution")
    log = (evidence / "native-test.log").read_text(encoding="utf-8", errors="replace")
    if not evidence_pass(log, code):
        raise RuntimeError("Native test did not prove one executed PASS; see native-test.log")
    report["test_binaries"] = [{"name": executable.name, "sha256": executable_hash}]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    evidence = args.evidence_dir.resolve()
    if evidence == repo or repo in evidence.parents:
        parser.error("Evidence directory must be outside the repository")
    evidence.mkdir(parents=True, exist_ok=False)
    report = {
        "source_sha": args.expected_head,
        "model_sha256": APPROVED_SHA256,
        "scope": "direct-backend hidden-device refusal only",
        "result": "HOLD",
    }
    try:
        run(repo, evidence, report)
        report["result"] = "PASS"
        return 0
    except Exception as error:
        report["error"] = str(error)
        return 2
    finally:
        (evidence / "receipt.json").write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )


if __name__ == "__main__":
    sys.exit(main())
