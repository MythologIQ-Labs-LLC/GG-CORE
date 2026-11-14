"""Join GG-CORE #184 stop-stage events with packaged stop-tail results.

Input: an output directory of n7_packaged_stop_tail.ps1 (results.jsonl plus
<tag>.err daemon stderr files). Daemons launched with GG_CORE_STOP_TRACE=1
write `gg_core::stop` JSON lines to stderr. Raw stderr also holds llama.cpp
native logs (including the model path): only `gg_core::stop` JSON lines are
read here, and raw .err files are never committed as evidence.

Per stop this prints daemon-clock stage deltas (S1 cancel received, S2
answered, S3 backend observed, S4 worker released), S4 -> CPU quiet on the
unix clock (S0 anchor), and per label/kind/config p50/p95/max. It then
evaluates the plan's pre-stated rules: S2->S3 <= last_decode_ms + 5 ms in
>= 95% of traced stops, identical uncancelled output across daemons, and
trace on/off throughput within 3%.

Usage: python -I n7_stop_stage_report.py <out-dir> [--rows]
"""

import json
import pathlib
import statistics
import sys

STOPS = ("prefillCancel", "prefillDeadline", "decodeCancel", "queuedCancel")


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
        fields["t"] = float(str(e["timestamp"]).strip().rstrip("s"))
        events.append(fields)
    return events


def to_unix(events, t):
    """Map a daemon uptime stamp to unix ms through S0 (`stop trace started`)."""
    s0 = [e for e in events if e.get("message") == "stop trace started"]
    return round(s0[0]["unix_ms"] + (t - s0[0]["t"]) * 1000, 1) if s0 else None


def stages_for(events, request_id, kind):
    msg = lambda e, m: e.get("message") == m and e.get("stream") is not True
    s1 = [e for e in events if msg(e, "cancel request received") and e.get("request_id") == request_id]
    s2 = [e for e in events if msg(e, "stop answered to caller") and e.get("request_id") == request_id]
    if not s2:
        return None
    qid, t2 = s2[0].get("queue_id"), s2[0]["t"]
    s3 = [e for e in events if msg(e, "backend stop observed") and e.get("queue_id") == qid and e["t"] >= t2 - 1]
    s4 = [e for e in events if msg(e, "worker released after stop") and e.get("queue_id") == qid and e["t"] >= t2 - 1]
    row = {"queue_id": qid, "kind": s2[0].get("kind"), "n_s3": len(s3), "n_s4": len(s4)}
    if s1:
        row["ack_ms"] = round((t2 - s1[0]["t"]) * 1000, 2)
    if s3:
        row.update(observe_ms=round((s3[0]["t"] - t2) * 1000, 1), phase=s3[0].get("phase"),
                   last_decode_ms=s3[0].get("last_decode_ms"), last_decode_tokens=s3[0].get("last_decode_tokens"))
    if s3 and s4:
        row["release_ms"] = round((s4[0]["t"] - s3[0]["t"]) * 1000, 1)
    if s4:
        row["s4_unix_ms"] = to_unix(events, s4[0]["t"])
    # Audit A2 / review C4: any dropped line in this daemon's trace, or a
    # stage that must exist but is missing, makes the join unsafe.
    any_drop = max((e.get("dropped", 0) for e in events), default=0) > 0
    missing = (kind != "queuedCancel" and not s4) or (kind == "queuedCancel" and (s3 or s4))
    row["incomplete"] = bool(any_drop or missing)
    row["queued_reached_backend"] = kind == "queuedCancel" and bool(s3 or s4)
    return row


def pct(values, q):
    values = sorted(v for v in values if v is not None)
    if not values:
        return None
    return values[max(0, min(len(values) - 1, round(q * (len(values) - 1))))]


def split_tag(tag):
    label, ctx, threads = tag.rsplit("-", 2)
    return label, f"{ctx}-{threads}"


def join(out, rows):
    cache, joined = {}, []
    for r in rows:
        if r.get("kind") not in STOPS:
            continue
        tag = r["tag"]
        if tag not in cache:
            err = out / f"{tag}.err"
            cache[tag] = load_events(err) if err.exists() else []
        st = stages_for(cache[tag], r.get("requestId"), r["kind"]) if cache[tag] else None
        if st and st.get("s4_unix_ms") and r.get("stopSentUnixMs") is not None:
            st["s4_to_quiet_ms"] = round(r["stopSentUnixMs"] + (r.get("tailWallMs") or 0) - st["s4_unix_ms"], 1)
        joined.append({**r, "stages": st})
    return joined


def summary(joined):
    groups = {}
    for j in joined:
        label, cfg = split_tag(j["tag"])
        groups.setdefault((label, j["kind"], cfg), []).append(j)
    print("label kind config n ok tail_p50 tail_p95 tail_max observe_max last_decode_max release_max s4_to_quiet_max")
    for (label, kind, cfg), js in sorted(groups.items()):
        tails = [j.get("tailWallMs") for j in js]
        st = [j["stages"] for j in js if j["stages"] and not j["stages"]["incomplete"]]
        ok = sum(1 for j in js if j.get("errorCode") in ("cancelled", "deadline_exceeded")
                 and (kind != "queuedCancel" or j.get("busyOk") == 2))
        col = lambda k: max((s[k] for s in st if s.get(k) is not None), default=None)
        print(label, kind, cfg, len(js), ok, pct(tails, .5), pct(tails, .95), max((t for t in tails if t is not None), default=None),
              col("observe_ms"), col("last_decode_ms"), col("release_ms"), col("s4_to_quiet_ms"))


def rules(rows, joined):
    st = [j["stages"] for j in joined if j["stages"]]
    print(f"\nruns excluded (dropped lines or missing stage): {sum(1 for s in st if s['incomplete'])}")
    print(f"queued cancels that reached the backend: {sum(1 for s in st if s['queued_reached_backend'])}")
    timed = [s for s in st if not s["incomplete"] and "observe_ms" in s and s.get("last_decode_ms") is not None]
    ok = sum(1 for s in timed if s["observe_ms"] <= s["last_decode_ms"] + 5)
    print(f"S2->S3 <= last_decode_ms + 5 ms: {ok}/{len(timed)}")
    tp = [r for r in rows if r.get("kind") == "throughput" and r.get("ok")]
    by_cfg = {}
    for r in tp:
        label, cfg = split_tag(r["tag"])
        by_cfg.setdefault(cfg, {}).setdefault(label, []).append(r)
    print("\nconfig label median_ms outputs")
    for cfg, labels in sorted(by_cfg.items()):
        for label, rs in sorted(labels.items()):
            print(cfg, label, statistics.median(r["ms"] for r in rs), sorted({r["outputSha"] for r in rs}))
        if "newon" in labels and "newoff" in labels:
            on = statistics.median(r["ms"] for r in labels["newon"])
            off = statistics.median(r["ms"] for r in labels["newoff"])
            print(f"  {cfg} trace on/off median ratio {on / off:.3f} (rule: within 0.97..1.03)")
    shas = {r["outputSha"] for r in tp}
    print(f"\ndistinct uncancelled output hashes across all daemons: {len(shas)} {sorted(shas)}")


def main(argv):
    out = pathlib.Path(argv[1])
    rows = [json.loads(x) for x in (out / "results.jsonl").read_text(encoding="utf-8-sig").splitlines() if x.strip()]
    joined = join(out, rows)
    if "--rows" in argv:
        for j in joined:
            print(json.dumps({k: j.get(k) for k in ("tag", "kind", "rep", "tailWallMs", "terminalMs",
                                                    "errorCode", "busyOk", "quiesced", "stages")}))
    summary(joined)
    rules(rows, joined)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
