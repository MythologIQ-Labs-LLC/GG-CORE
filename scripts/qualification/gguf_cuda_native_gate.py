#!/usr/bin/env python3
"""Pinned offline Windows CUDA native execution for GG-CORE #224.

PASS means the real GPU test binaries executed successfully, not that GG-CORE
has achieved release parity, process-attributed VRAM recovery, IPC or P0 proof.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

import gguf_cuda_hidden_device_gate as common

FIXTURE_BYTES = 491_400_032
FIXTURE_TEST = "common::fixture_integrity_tests::pinned_fixture_check_rejects_wrong_bytes_and_wrong_hash"
GPU_TESTS = (
    "q1_gpu_greedy_output_characterization_against_cpu",
    "q2_first_token_latency_and_throughput_cpu_vs_gpu",
    "q3_gpu_stream_cancellation_stops_and_model_stays_usable",
    "q4_cpu_gpu_transition_vram_characterization_only",
    "q5_concurrent_gpu_generations_match_serial",
    FIXTURE_TEST,
)
OOM_TESTS = ("oversized_gpu_context_is_refused_then_a_normal_load_works", FIXTURE_TEST)
GPU_MARKERS = (
    "GPUQUAL correctness.p0.gpu=",
    "GPUQUAL perf.cpu.run0.tokens=",
    "GPUQUAL perf.gpu.run0.tokens=",
    "GPUQUAL cancel.stop_observed=true",
    "GPUQUAL vram.telemetry=AGGREGATE_DEVICE_ONLY",
    "GPUQUAL concurrent.round0.matches_serial=true",
    "GPUQUAL concurrent.round1.matches_serial=true",
    "GPUQUAL concurrent.round2.matches_serial=true",
    "GPUQUAL correctness.release_qualification=NOT_QUALIFIED",
    "GPUQUAL vram.release_qualification=NOT_QUALIFIED",
)
OOM_MARKERS = ("GPUQUAL oom.refusal=PASS",)
UUID = re.compile(r"GPU-[0-9A-Za-z-]{8,}")


def pinned_gpu_information(output, uuid, memory_mib):
    rows = [line.strip() for line in output.splitlines() if line.strip()]
    if len(rows) != 1:
        raise RuntimeError("GPU query must return exactly one device")
    fields = [field.strip() for field in rows[0].split(",")]
    if len(fields) != 4 or fields[0] != uuid or not UUID.fullmatch(uuid):
        raise RuntimeError("GPU UUID does not match pinned device")
    try:
        actual_mib = int(fields[2])
    except ValueError as exc:
        raise RuntimeError("NVIDIA GPU capacity is not numeric") from exc
    if not (0 < actual_mib <= 16384) or actual_mib != memory_mib:
        raise RuntimeError("GPU memory differs from explicit <=16 GiB test target")
    if not fields[1] or not fields[3]:
        raise RuntimeError("GPU model/driver identity missing")
    return dict(uuid=uuid, model=fields[1], memory_mib=actual_mib,
                driver_version=fields[3])


def executed_tests_pass(log, returncode, names, markers):
    lines = log.splitlines()
    summary = f"test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"
    return (returncode == 0
            and sum(summary in line for line in lines) == 1
            and all(any(f"test {name} ... ok" in line for line in lines) for name in names)
            and all(any(marker in line for line in lines) for marker in markers))


def run_suite(repo, evidence, target, env, binary, names, markers):
    cmd = ["cargo", "test", "--locked", "--offline", "--features",
           "gguf-cuda", "--test", binary, "--", "--nocapture", "--test-threads=1"]
    log_path = evidence / (binary + ".log")
    began = time.monotonic()
    with log_path.open("w", encoding="utf-8") as output:
        try:
            result = subprocess.run(cmd, cwd=repo / "core-runtime", env=env,
                                    stdout=output, stderr=subprocess.STDOUT,
                                    timeout=3600, check=False)
            code = result.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            output.write(f"Native test invocation failed: {error}\n")
            code = 127
    log = log_path.read_text(encoding="utf-8", errors="replace")
    if not executed_tests_pass(log, code, names, markers):
        raise RuntimeError(f"{binary}: required real tests/markers failed; inspect {log_path.name}")
    binaries = sorted((target / "debug" / "deps").glob(binary + "-*.exe"))
    if not binaries:
        raise RuntimeError(f"{binary}: current-attempt Windows binary was not found")
    return {
        "command": cmd, "exit_code": code, "executed_tests": len(names),
        "elapsed_seconds": round(time.monotonic() - began, 3),
        "log": log_path.name,
        "binary_hashes": [
            {"file": path.name, "sha256": common.digest(path)} for path in binaries
        ],
    }


def run(repo, evidence, report, gpu_uuid, memory_mib, smi):
    common.validate(repo, report["source_sha"])
    fixture = repo / common.MODEL
    if fixture.is_symlink() or not fixture.is_file() or fixture.stat().st_size != FIXTURE_BYTES:
        raise RuntimeError("Approved pinned GGUF fixture must be a regular file of exact size")
    common.require_offline_windows()
    if not smi.is_absolute() or not smi.is_file():
        raise RuntimeError("nvidia-smi must be a verified absolute executable path")
    info = common.checked([
        str(smi), "-i", gpu_uuid, "--query-gpu=uuid,name,memory.total,driver_version",
        "--format=csv,noheader,nounits",
    ])
    report["gpu"] = pinned_gpu_information(info, gpu_uuid, memory_mib)
    report["toolchain"] = {
        "rustc": common.checked(["rustc", "-Vv"]),
        "cargo": common.checked(["cargo", "-V"]),
    }
    target = evidence / "cargo-target"
    target.mkdir()
    env = dict(os.environ)
    env.update({
        "CUDA_VISIBLE_DEVICES": gpu_uuid, "GG_GPU_DEVICE_UUID": gpu_uuid,
        "GG_NVSMI": str(smi), "CARGO_TARGET_DIR": str(target.resolve()),
        "CARGO_NET_OFFLINE": "true", "GG_CORE_REQUIRE_REAL_MODEL": "1",
    })
    report["environment_controls"] = {
        "CUDA_VISIBLE_DEVICES": gpu_uuid, "GG_GPU_DEVICE_UUID": gpu_uuid,
        "CARGO_NET_OFFLINE": "true", "GG_CORE_REQUIRE_REAL_MODEL": "1",
    }
    report["suites"] = {}
    for name, tests, markers in (
        ("gguf_cuda_qualification_test", GPU_TESTS, GPU_MARKERS),
        ("gguf_cuda_oom_test", OOM_TESTS, OOM_MARKERS),
    ):
        report["suites"][name] = run_suite(repo, evidence, target, env, name, tests, markers)
    report["result"] = "EXECUTION_PASS_RELEASE_HOLD"
    report["release_qualification"] = (
        "NOT_QUALIFIED: no approved CPU/GPU parity tolerance or performance "
        "threshold; no process-attributed VRAM recovery, authenticated IPC, "
        "P0 #219 native proof, or owner GO"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--gpu-uuid", required=True)
    parser.add_argument("--expected-total-mib", type=int, required=True)
    parser.add_argument("--nvidia-smi", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    evidence = args.evidence_dir.resolve()
    if evidence == repo or repo in evidence.parents:
        parser.error("Evidence directory must be outside the repository")
    evidence.mkdir(parents=True, exist_ok=False)
    report = {
        "source_sha": args.expected_head,
        "model_sha256": common.APPROVED_SHA256,
        "scope": "offline Windows native CUDA test execution; release HOLD",
        "result": "HOLD",
    }
    try:
        run(repo, evidence, report, args.gpu_uuid, args.expected_total_mib,
            args.nvidia_smi.resolve())
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
