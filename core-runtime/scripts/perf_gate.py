#!/usr/bin/env python3
"""Run-over-run perf-regression gate (B-34b).

Compares two criterion output trees and fails if any bench's median regressed
beyond a threshold ratio.

GitHub's `ubuntu-latest` pool spans machine classes, so the baseline and the
current run may come from different hardware. A slower machine shifts EVERY
bench by a similar factor (observed on PR #127: a docs-only diff "regressed"
8 benches, run-wide median ratio ~1.5x, while some benches got faster). To
stay sound across that, the gate computes the run-wide median ratio over all
gated benches and, when the fleet shifted slower (median > 1), gates each
bench on its ratio NORMALIZED by that shift: a real regression still sticks
out relative to the rest of the run; a uniform hardware delta does not fail.

Usage: perf_gate.py <baseline_dir> <current_dir> <threshold>
  threshold: max allowed shift-normalized median ratio (e.g. 2.0 = +100%).
A missing/empty baseline (cache miss, first run) is NOT a regression -> exit 0.
A bench present now but absent from the baseline (new bench) is reported, not failed.
"""
import json
import os
import sys

# Below this baseline median, CI-trimmed criterion (--measurement-time 2 --sample-size 10)
# variance exceeds the regression threshold, so those benches are reported but not gated
# (a 2x "regression" on a sub-microsecond bench is scheduler jitter, not code). B-34c;
# observed on PR #101 (concurrent_resource_ops ~83-321 ns swinging 2.3-2.4x between runners).
NOISE_FLOOR_NS = 1000.0


def medians(root):
    """Map "<group>/<id>" -> median point estimate (ns) for each criterion bench."""
    out = {}
    for dirpath, _dirs, files in os.walk(root):
        if os.path.basename(dirpath) == "new" and "estimates.json" in files:
            key = os.path.relpath(os.path.dirname(dirpath), root).replace(os.sep, "/")
            with open(os.path.join(dirpath, "estimates.json")) as fh:
                out[key] = json.load(fh)["median"]["point_estimate"]
    return out


def run_median(values):
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2.0


def main(argv):
    if len(argv) != 4:
        print("usage: perf_gate.py <baseline_dir> <current_dir> <threshold>")
        return 2
    baseline_dir, current_dir, threshold = argv[1], argv[2], float(argv[3])

    base = medians(baseline_dir)
    cur = medians(current_dir)

    if not base:
        print(f"perf-gate: no baseline at '{baseline_dir}' (cache miss / first run) -> skip.")
        return 0

    # Pass 1: collect ratios for gated (>= floor) benches to estimate the
    # run-wide shift between the baseline's runner and this one.
    gated = []
    for key, cur_ns in cur.items():
        base_ns = base.get(key)
        if base_ns and base_ns >= NOISE_FLOOR_NS:
            gated.append((key, base_ns, cur_ns, cur_ns / base_ns))

    shift = max(run_median([r for (_, _, _, r) in gated]), 1.0) if gated else 1.0
    if shift > 1.0:
        print(
            f"perf-gate: run-wide median ratio x{shift:.2f} across {len(gated)} gated "
            f"bench(es) -- normalizing for runner-class shift (gating on ratio/shift)"
        )

    # Pass 2: report everything; gate on shift-normalized ratios.
    regressions = []
    normalized = {key: ratio / shift for (key, _, _, ratio) in gated}
    for key, cur_ns in sorted(cur.items()):
        base_ns = base.get(key)
        if base_ns is None:
            print(f"  NEW        {key}: {cur_ns:.1f} ns (no baseline)")
            continue
        ratio = cur_ns / base_ns if base_ns else float("inf")
        if base_ns < NOISE_FLOOR_NS:
            # Sub-floor: report only; CI jitter dominates, not gated (B-34c).
            print(
                f"  noisy      {key}: {base_ns:.1f} -> {cur_ns:.1f} ns "
                f"(x{ratio:.2f}; < {NOISE_FLOOR_NS:.0f}ns floor -- not gated)"
            )
            continue
        eff = normalized[key]
        flag = "REGRESSION" if eff > threshold else "ok"
        norm_note = f", x{eff:.2f} shift-normalized" if shift > 1.0 else ""
        print(f"  {flag:10} {key}: {base_ns:.1f} -> {cur_ns:.1f} ns (x{ratio:.2f}{norm_note})")
        if eff > threshold:
            regressions.append(key)

    if regressions:
        print(f"perf-gate: FAIL -- {len(regressions)} bench(es) regressed > {threshold:.2f}x")
        return 1
    print(f"perf-gate: PASS -- no median regression > {threshold:.2f}x (shift x{shift:.2f})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
