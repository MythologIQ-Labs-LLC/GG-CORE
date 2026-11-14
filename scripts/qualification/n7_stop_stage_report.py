"""Join GG-CORE #184 stop-stage events with packaged stop-tail results.

Input: an output directory of n7_packaged_stop_tail.ps1 (results.jsonl plus
<tag>.err daemon stderr files). Daemons launched with GG_CORE_STOP_TRACE=1
write `gg_core::stop` JSON lines to stderr; other daemons write none.

Per stop scenario this prints the daemon-clock stage deltas (S1 cancel
received, S2 answered, S3 backend observed, S4 worker released) and the
harness-measured CPU quiescence, then p50/p95/max per kind and config.
No prompt text exists in either input.

Usage: python -I n7_stop_stage_report.py <out-dir>
"""

import json
import pathlib
import statistics
import sys


def uptime_s(stamp):
    # tracing_subscriber Uptime timer: "12.345678s"
    return float(str(stamp).strip().rstrip("s"))


def load_events(err_path):
    events = []
    for line in err_path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if e.get("target") != "gg_core::stop":
            continue
        fields = dict(e.get("fields", {}))
        fields.update(e.get("span", {}) or {})
        fields["t"] = uptime_s(e["timestamp"])
        events.append(fields)
    return events


def stages_for(events, request_id):
    s1 = [e for e in events if e.get("message") == "cancel request received"
          and e.get("request_id") == request_id]
    s2 = [e for e in events if e.get("message") == "stop answered to caller"
          and e.get("request_id") == request_id]
    if not s2:
        return None
    qid = s2[0].get("queue_id")
    s3 = [e for e in events if e.get("message") == "backend stop observed" and e.get("queue_id") == qid]
    s4 = [e for e in events if e.get("message") == "worker released after stop" and e.get("queue_id") == qid]
    t2 = s2[0]["t"]
    row = {"queue_id": qid, "kind": s2[0].get("kind"), "n_s3": len(s3), "n_s4": len(s4)}
    if s1:
        row["ack_ms"] = round((t2 - s1[0]["t"]) * 1000, 2)
    if s3:
        row["observe_ms"] = round((s3[0]["t"] - t2) * 1000, 1)
        row["phase"] = s3[0].get("phase")
        row["last_decode_ms"] = s3[0].get("last_decode_ms")
        row["last_decode_tokens"] = s3[0].get("last_decode_tokens")
    if s3 and s4:
        row["release_ms"] = round((s4[0]["t"] - s3[0]["t"]) * 1000, 1)
    if s4:
        row["s4_unix_ms"] = to_unix(events, s4[0]["t"])
    # Audit A2: any line dropped by the daemon's writer makes the join unsafe.
    row["incomplete"] = any(e.get("dropped", 0) for e in s1 + s2 + s3 + s4)
    return row


def to_unix(events, t):
    """Map a daemon uptime stamp to unix ms through S0 (`stop trace started`)."""
    s0 = [e for e in events if e.get("message") == "stop trace started"]
    if not s0:
        return None
    return round(s0[0]["unix_ms"] + (t - s0[0]["t"]) * 1000, 1)


def pct(values, q):
    values = sorted(values)
    if not values:
        return None
    k = max(0, min(len(values) - 1, round(q * (len(values) - 1))))
    return values[k]


def main(out_dir):
    out = pathlib.Path(out_dir)
    rows = [json.loads(x) for x in (out / "results.jsonl").read_text(encoding="utf-8").splitlines() if x.strip()]
    cache = {}
    joined = []
    for r in rows:
        if r.get("kind") not in ("prefillCancel", "prefillDeadline", "decodeCancel", "queuedCancel"):
            continue
        tag = r["tag"]
        if tag not in cache:
            err = out / f"{tag}.err"
            cache[tag] = load_events(err) if err.exists() else []
        st = stages_for(cache[tag], r.get("requestId")) if cache[tag] else None
        if st and st.get("s4_unix_ms") and r.get("stopSentUnixMs") is not None:
            quiet = r["stopSentUnixMs"] + (r.get("tailWallMs") or 0)
            st["s4_to_quiet_ms"] = round(quiet - st["s4_unix_ms"], 1)
        joined.append({**r, "stages": st})
    for j in joined:
        print(json.dumps({k: j.get(k) for k in ("tag", "kind", "rep", "tailWallMs", "terminalMs",
                                                "errorCode", "busyOk", "quiesced", "stages")}))
    groups = {}
    for j in joined:
        label, ctx_t = j["tag"].split("-", 1)
        groups.setdefault((label, j["kind"], ctx_t), []).append(j)
    excluded = sum(1 for j in joined if j["stages"] and j["stages"]["incomplete"])
    print(f"\nruns excluded for dropped trace lines: {excluded}")
    usable = [j["stages"] for j in joined if j["stages"] and not j["stages"]["incomplete"]]
    timed = [s for s in usable if "observe_ms" in s and s.get("last_decode_ms") is not None]
    ok = sum(1 for s in timed if s["observe_ms"] <= s["last_decode_ms"] + 5)
    print(f"S2->S3 <= last_decode_ms + 5 ms: {ok}/{len(timed)}")
    print("\nlabel kind config n tail_p50 tail_p95 tail_max observe_max last_decode_max release_max")
    for (label, kind, cfg), js in sorted(groups.items()):
        tails = [j["tailWallMs"] for j in js if j.get("tailWallMs") is not None]
        st = [j["stages"] for j in js if j["stages"] and not j["stages"]["incomplete"]]
        obs = [s["observe_ms"] for s in st if "observe_ms" in s]
        ld = [s["last_decode_ms"] for s in st if s.get("last_decode_ms") is not None]
        rel = [s["release_ms"] for s in st if "release_ms" in s]
        print(label, kind, cfg, len(js), pct(tails, .5), pct(tails, .95), max(tails, default=None),
              max(obs, default=None), max(ld, default=None), max(rel, default=None))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
