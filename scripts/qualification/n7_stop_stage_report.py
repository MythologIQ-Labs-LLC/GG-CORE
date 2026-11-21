"""Fail-closed evidence gate for GG-CORE #184 stop-stage measurements.

Input: an output directory of n7_packaged_stop_tail.ps1 (results.jsonl plus
<tag>.err daemon stderr), or --trace FILE: the committed filtered trace
(`N7_STOP_STAGE_TRACE_*.jsonl`, one parsed `gg_core::stop` event per line
with its `tag`). Raw stderr also holds llama.cpp native logs (including the
model path): only `gg_core::stop` JSON lines are ever read, and raw .err
files are never committed.

Output: DESCRIPTIVE statistics (every run, incomplete traces counted and
kept out of the stage columns), then the ACCEPTANCE rules, evaluated on
complete traced runs only. The rules are the ones pre-stated in
N7_STOP_STAGE_TRACE_PLAN.md (Revision 2 F6/F7, audit A2, review C4); none
is stricter:
  R1 no dropped trace lines and no missing required stage in traced runs
  R2 no queued cancel reached the backend (no S3/S4)
  R3 the full matrix is present: every label x config x stop kind has at
     least --reps runs, every traced-label stop was joined, recoveries ran
  R4 every stop ended correctly (explicit error code, cancel ACK + one
     terminal frame, quiesced; queued: both busy requests succeeded) and
     every recovery request succeeded on a live daemon
  R5 S2->S3 <= last_decode_ms + 5 ms (0 + 5 ms for a setup-phase stop) in
     >= 95% of traced running stops; every traced running stop must be timed
     (exactly one S2, S3 and S4) and every running-stop cell represented
  R6 trace on/off uncancelled throughput median ratio within 0.97..1.03
  R7 exactly one uncancelled output hash per config across --hash-labels
  R8 (optional --require-stream) each label/config/rep has a real active
     streaming cancel: first text before stop, exactly one ACK and failed
     terminal in either order, no data after ACK/final, no late frame,
     and a conservative CPU-tail upper bound (not an exact stop instant).
Historical nonstreaming R1-R7 evidence remains valid without R8; passing
R8 does NOT substitute for native streaming S1-S4 stop-stage trace joins.
Exit 0 = every rule passes, 1 = a rule failed, 2 = malformed/missing input.
Joins are valid only for the harness as run: one client, one stop at a
time, non-streaming (review N7-R2). Lines dropped at the very end of a
daemon's trace leave no later `dropped` count; R1/R5 then catch them as
missing stages. Duplicate (tag, requestId) rows and settings that would make
a rule vacuous (same on/off label, reps < 1, labels missing) exit 2.

Usage: python -I n7_stop_stage_report.py <out-dir> [--trace FILE] [--results FILE]
       [--labels c1,main,newoff,newon] [--trace-label newon]
       [--off-label newoff] [--hash-labels main,newoff,newon]
       [--contexts 8192,16384] [--threads 0,1,2] [--reps 3] [--rows]
"""

import argparse
import json
import math
import pathlib
import statistics
import sys

STOPS = ("prefillCancel", "prefillDeadline", "decodeCancel", "queuedCancel")
RUNNING = ("prefillCancel", "prefillDeadline", "decodeCancel")
CODES = {"prefillCancel": "cancelled", "decodeCancel": "cancelled",
         "queuedCancel": "cancelled", "prefillDeadline": "deadline_exceeded"}
FRAMES = "cancel_response,inference_response"


class BadInput(Exception):
    """Malformed or missing evidence (exit 2)."""



def validate_trace_fields(event):
    """Reject untrusted stage payloads that could corrupt acceptance arithmetic."""
    for field in ("request_id", "queue_id", "last_decode_tokens", "dropped", "unix_ms"):
        value = event.get(field)
        if value is not None and (type(value) is not int or value < 0):
            raise BadInput(f"stop-trace field {field} must be a nonnegative integer")
    value = event.get("last_decode_ms")
    if value is not None and (type(value) not in (int, float)
                              or not math.isfinite(value) or value < 0):
        raise BadInput("stop-trace last_decode_ms must be finite and nonnegative")
    for field in ("matched", "stream"):
        if field in event and type(event[field]) is not bool:
            raise BadInput(f"stop-trace field {field} must be boolean")


def parse_err(path):
    events = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            e = json.loads(line)
        except ValueError:
            continue  # native / eprintln lines are not stop-trace evidence
        if e.get("target") != "gg_core::stop":
            continue
        fields = dict(e.get("fields", {}))
        fields.update(e.get("span", {}) or {})
        try:
            fields["t"] = float(str(e["timestamp"]).strip().rstrip("s"))
        except (KeyError, ValueError, TypeError) as err:
            raise BadInput(f"{path.name}: stop-trace line without a valid timestamp ({err})") from err
        if not math.isfinite(fields["t"]):
            raise BadInput(f"{path.name}: stop-trace timestamp must be finite")
        validate_trace_fields(fields)
        events.append(fields)
    return events


def load_jsonl(path, what):
    if not path.exists():
        raise BadInput(f"missing {what}: {path}")
    rows = []
    for n, line in enumerate(path.read_text(encoding="utf-8-sig").splitlines(), 1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except ValueError as e:
            raise BadInput(f"{path.name}:{n}: not JSON ({e})") from e
    if not rows:
        raise BadInput(f"empty {what}: {path}")
    return rows


def load_traces(out, trace_file, rows):
    if trace_file:
        traces = {}
        for e in load_jsonl(pathlib.Path(trace_file), "trace file"):
            if not isinstance(e, dict) or not {"tag", "message", "t"} <= e.keys():
                raise BadInput(f"trace event without tag/message/t: {e}")
            if (isinstance(e["t"], bool) or not isinstance(e["t"], (int, float))
                    or not math.isfinite(e["t"])):
                raise BadInput(f"trace event without finite numeric t: {e}")
            validate_trace_fields(e)
            traces.setdefault(e["tag"], []).append(e)
        return traces
    tags = {r["tag"] for r in rows if r.get("kind") in STOPS}
    return {t: parse_err(out / f"{t}.err") for t in tags if (out / f"{t}.err").exists()}


def validate(rows):
    seen = set()
    for r in rows:
        if not isinstance(r, dict) or "kind" not in r:
            raise BadInput(f"row without kind: {r}")
        if r["kind"] in STOPS + ("throughput", "recover", "streamCancel"):
            if not isinstance(r.get("tag"), str) or r["tag"].count("-") < 2:
                raise BadInput(f"row with malformed tag: {r}")
        if r["kind"] in STOPS + ("throughput", "recover", "streamCancel") and (type(r.get("rep")) is not int or r["rep"] < 0):
            raise BadInput(f"measurement row without nonnegative integer repetition: {r}")
        if r["kind"] in STOPS + ("streamCancel",) and (type(r.get("requestId")) is not int or r["requestId"] < 0):
            raise BadInput(f"stop row without integer requestId: {r}")
        if r["kind"] in STOPS + ("streamCancel",):
            key = (r["tag"], r["requestId"])
            if key in seen:
                raise BadInput(f"duplicate stop row for {key}")
            seen.add(key)
        if r["kind"] == "throughput" and r.get("ok"):
            ms = r.get("ms")
            if type(ms) not in (int, float) or not math.isfinite(ms) or ms <= 0:
                raise BadInput(f"throughput row without positive finite ms: {r}")


def split_tag(tag):
    label, ctx, threads = tag.rsplit("-", 2)
    return label, f"{ctx}-{threads}"


def to_unix(events, t):
    s0 = [e for e in events if e.get("message") == "stop trace started"]
    return round(s0[0]["unix_ms"] + (t - s0[0]["t"]) * 1000, 1) if s0 else None


def stages_for(events, request_id, kind):
    """Join one single-client, non-streaming stop; refuse impossible chronology."""
    msg = lambda e, m: e.get("message") == m and e.get("stream") is not True
    s1 = [e for e in events if msg(e, "cancel request received") and e.get("request_id") == request_id]
    s2 = [e for e in events if msg(e, "stop answered to caller") and e.get("request_id") == request_id]
    if len(s2) != 1:
        return {"incomplete": True, "missing": "S2" if not s2 else "duplicate S2", "queued_reached_backend": False}
    qid, t2 = s2[0].get("queue_id"), s2[0]["t"]
    # No one-second lookback: a prior S3 cannot demonstrate a later stop.
    s3 = [e for e in events if msg(e, "backend stop observed") and e.get("queue_id") == qid]
    s4 = [e for e in events if msg(e, "worker released after stop") and e.get("queue_id") == qid]
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
    any_drop = max((e.get("dropped", 0) for e in events), default=0) > 0
    reached = kind == "queuedCancel" and bool(s3 or s4)
    no_s3 = kind != "queuedCancel" and len(s3) != 1
    no_s4 = kind != "queuedCancel" and len(s4) != 1
    explicit = kind != "prefillDeadline"
    no_s1 = explicit and (len(s1) != 1 or s1[0].get("matched") is not True)
    wrong_kind = s2[0].get("kind") != CODES[kind] or not isinstance(qid, int)
    bad_order = (explicit and len(s1) == 1 and s1[0]["t"] > t2
                 or len(s3) == 1 and s3[0]["t"] < t2
                 or len(s3) == 1 and len(s4) == 1 and s4[0]["t"] < s3[0]["t"])
    row["incomplete"] = bool(any_drop or no_s1 or no_s3 or no_s4 or reached or bad_order or wrong_kind)
    row["missing"] = ("dropped lines" if any_drop else "invalid S1" if no_s1
                      else "stage order" if bad_order else "kind or queue" if wrong_kind
                      else "S3" if no_s3 else "S4" if no_s4 else None)
    row["queued_reached_backend"] = reached
    return row


def join(rows, traces, trace_label):
    joined = []
    for r in rows:
        if r.get("kind") not in STOPS:
            continue
        st = None
        if split_tag(r["tag"])[0] == trace_label:
            st = stages_for(traces.get(r["tag"], []), r["requestId"], r["kind"])
            # Measure from the actual CPU-sampling start, not the terminal or
            # a guessed deadline timestamp. Historical receipts without this
            # field get no derived S4-to-quiet statistic (rather than fiction).
            quiet_started = r.get("tailClockOriginUnixMs")
            if st.get("s4_unix_ms") and type(quiet_started) is int:
                st["s4_to_quiet_ms"] = round(quiet_started + (r.get("tailWallMs") or 0) - st["s4_unix_ms"], 1)
        joined.append({**r, "stages": st})
    return joined


def pct(values, q):
    values = sorted(v for v in values if v is not None)
    return values[max(0, min(len(values) - 1, round(q * (len(values) - 1))))] if values else None


def describe(joined):
    print("DESCRIPTIVE (all runs; stage columns from complete traces only)")
    print("label kind config n tail_p50 tail_p95 tail_max incomplete observe_max last_decode_max release_max s4_to_quiet_max")
    groups = {}
    for j in joined:
        label, cfg = split_tag(j["tag"])
        groups.setdefault((label, j["kind"], cfg), []).append(j)
    for (label, kind, cfg), js in sorted(groups.items()):
        tails = [j.get("tailWallMs") for j in js]
        st = [j["stages"] for j in js if j["stages"]]
        ok = [s for s in st if not s["incomplete"]]
        col = lambda k: max((s[k] for s in ok if s.get(k) is not None), default=None)
        print(label, kind, cfg, len(js), pct(tails, .5), pct(tails, .95), max((t for t in tails if t is not None), default=None),
              len(st) - len(ok), col("observe_ms"), col("last_decode_ms"), col("release_ms"), col("s4_to_quiet_ms"))


def stop_ok(j):
    if j.get("errorCode") != CODES[j["kind"]]:
        return False
    if j["kind"] != "prefillDeadline" and j.get("frames") != FRAMES:
        return False
    if j["kind"] == "queuedCancel":
        return j.get("busyOk") == 2
    return j.get("quiesced") is True


def r3_matrix(joined, rows, a, configs):
    """Require exactly one record for each declared label/config/kind/rep."""
    expected = {(l, c, k, rep) for l in a.labels for c in configs
                for k in (*STOPS, "throughput", "recover") for rep in range(a.reps)}
    counts = {}
    for r in rows:
        if r["kind"] not in (*STOPS, "throughput", "recover"):
            continue
        l, c = split_tag(r["tag"])
        key = (l, c, r["kind"], r.get("rep"))
        counts[key] = counts.get(key, 0) + 1
    missing = expected - set(counts)
    extra = set(counts) - expected
    duplicate = {k for k, n in counts.items() if n != 1}
    traced = sum(1 for j in joined if j["stages"] is not None)
    required_traced = len(configs) * a.reps * len(STOPS)
    ok = not (missing or extra or duplicate) and traced == required_traced
    detail = (f"missing {sorted(map(str, missing))[:6]}; "
              f"extra {sorted(map(str, extra))[:6]}; "
              f"duplicate {sorted(map(str, duplicate))[:6]}; "
              f"traced {traced}/{required_traced}")
    return ok, detail


def r5_observe(traced, joined, a, configs):
    running = [j for j in joined if j["stages"] is not None and j["kind"] in RUNNING]
    timed = [(j, j["stages"]) for j in running if not j["stages"]["incomplete"] and "observe_ms" in j["stages"]]
    # A setup-phase stop had no decode in flight: it must be observed within 0 + 5 ms.
    within = sum(1 for _, s in timed if s["observe_ms"] <= (s.get("last_decode_ms") or 0) + 5)
    cells = {(split_tag(j["tag"])[1], j["kind"]) for j, _ in timed}
    need = {(c, k) for c in configs for k in RUNNING}
    ok = bool(timed) and len(timed) == len(running) and not (need - cells) and within >= 0.95 * len(timed)
    return ok, (f"{within}/{len(timed)} within; {len(running) - len(timed)} traced running stops untimed; "
                f"cells without a timed stop: {sorted(need - cells)[:4]}")


def throughput(rows, a, configs):
    tp = {}
    for r in rows:
        if r.get("kind") == "throughput":
            label, cfg = split_tag(r["tag"])
            tp.setdefault(cfg, {}).setdefault(label, []).append(r)
    ratios, hashes, short = {}, {}, []
    for cfg in configs:
        for l in sorted({a.trace_label, a.off_label, *a.hash_labels}):
            good = [r for r in tp.get(cfg, {}).get(l, []) if r.get("ok") and r.get("outputSha")]
            if len(good) < a.reps:
                short.append(f"{cfg}/{l}={len(good)}")
        on = [r["ms"] for r in tp.get(cfg, {}).get(a.trace_label, []) if r.get("ok")]
        off = [r["ms"] for r in tp.get(cfg, {}).get(a.off_label, []) if r.get("ok")]
        ratios[cfg] = statistics.median(on) / statistics.median(off) if on and off else None
        hashes[cfg] = {r.get("outputSha") for l in a.hash_labels for r in tp.get(cfg, {}).get(l, [])}
    r6 = not short and all(v is not None and 0.97 <= v <= 1.03 for v in ratios.values())
    r7 = not short and all(len(h) == 1 and None not in h for h in hashes.values())
    d6 = " ".join(f"{c}={'none' if v is None else f'{v:.3f}'}" for c, v in ratios.items())
    if short:
        d6 += f"; throughput rows short: {short[:6]}"
    d7 = " ".join(f"{c}={sorted(map(str, h))}" for c, h in hashes.items())
    if short:
        d7 += f"; throughput rows short: {short[:6]}"
    return (r6, d6), (r7, d7)


def r8_stream(rows, a, configs):
    """Fail-closed real-stream receipt matrix; no fabricated trace claims."""
    expected = {(label, cfg, rep) for label in a.labels for cfg in configs
                for rep in range(a.reps)}
    seen = {}
    invalid = []
    for row in rows:
        if row.get("kind") != "streamCancel":
            continue
        label, cfg = split_tag(row["tag"])
        key = (label, cfg, row["rep"])
        seen[key] = seen.get(key, 0) + 1
        def integer(name, value):
            return type(value) is int and value == row.get(name)
        def ms(name):
            value = row.get(name)
            return (type(value) in (int, float) and math.isfinite(value)
                    and value >= 0)
        # Receipts require actual generated TEXT before cancel, not merely
        # an idle queue cancellation. Positive and negative evidence are
        # explicit: ACK exactly once, failed terminal exactly once, no
        # output after ACK, no late frames, no unexpected IDs/frames, and
        # measured CPU stop. All timing comes from the same cancel stopwatch.
        ok = (
            row.get("started") is True
            and integer("prefixFrames", row.get("prefixFrames"))
            and row["prefixFrames"] >= 1
            and ms("firstTextMs")
            and row.get("ack") is True
            and integer("ackCount", 1)
            and integer("terminalCount", 1)
            # Terminal may race ahead of ACK on the same connection.
            and isinstance(row.get("terminalError"), str)
            and "cancel" in row["terminalError"].lower()
            and integer("lateFrames", 0)
            and integer("dataAfterAck", 0)
            and integer("dataAfterFinal", 0)
            and integer("unexpectedFrames", 0)
            and row.get("readOutcome") == "OK"
            and row.get("postTerminalRead") == "TIMEOUT"
            and row.get("quiesced") is True
            and ms("ackMs") and ms("terminalMs") and ms("tailWallMs")
            and row.get("tailBound") == "conservativeUpperBound"
            and ms("cpuObservationStartMs")
            and row["tailWallMs"] >= row["cpuObservationStartMs"]
            and type(row.get("stopSentUnixMs")) is int
            and row["stopSentUnixMs"] > 0
            and type(row.get("tailClockOriginUnixMs")) is int
            and row["tailClockOriginUnixMs"] == row["stopSentUnixMs"]
            and type(row.get("cpuObservationStartUnixMs")) is int
            and row["cpuObservationStartUnixMs"] >= row["stopSentUnixMs"]
        )
        if not ok:
            invalid.append(str(key))
    missing = expected - set(seen)
    extra = set(seen) - expected
    duplicates = {key for key, count in seen.items() if count != 1}
    ok = not (missing or extra or duplicates or invalid)
    detail = (f"qualified {len(expected) - len(missing) - len(duplicates) - len(invalid)}/{len(expected)}; "
              f"missing {sorted(map(str, missing))[:6]}; "
              f"unexpected {sorted(map(str, extra))[:6]}; "
              f"duplicate {sorted(map(str, duplicates))[:6]}; "
              f"invalid {invalid[:6]}")
    return ok, detail


def rules(rows, joined, a):
    configs = [f"ctx{c}-t{t}" for c in a.contexts for t in a.threads]
    traced = [j["stages"] for j in joined if j["stages"] is not None]
    bad_stops = [f"{j['tag']}/{j['kind']}/rep{j.get('rep')}" for j in joined if not stop_ok(j)]
    bad_recov = [r["tag"] for r in rows if r.get("kind") == "recover" and not (r.get("ok") and r.get("alive"))]
    incomplete = [s for s in traced if s["incomplete"]]
    reached = [s for s in traced if s["queued_reached_backend"]]
    r6, r7 = throughput(rows, a, configs)
    results = [
        ("R1 no dropped lines or missing stages in traced runs", not incomplete,
         f"{len(incomplete)} incomplete of {len(traced)} traced"),
        ("R2 no queued cancel reached the backend", not reached, f"{len(reached)} reached"),
        ("R3 full matrix present and traced", *r3_matrix(joined, rows, a, configs)),
        ("R4 every stop and recovery correct", not bad_stops and not bad_recov,
         f"bad stops {bad_stops[:6]}; bad recoveries {bad_recov[:6]}"),
        ("R5 S2->S3 <= last_decode_ms + 5 ms in >= 95% of timed stops", *r5_observe(traced, joined, a, configs)),
        ("R6 trace on/off throughput median ratio within 0.97..1.03", *r6),
        ("R7 one uncancelled output hash per config across hash labels", *r7),
    ]
    if a.require_stream:
        results.append(("R8 real active streaming cancel/no late frames/CPU quiescence",
                        *r8_stream(rows, a, configs)))
    print("\nACCEPTANCE (complete traced runs only)")
    for name, ok, detail in results:
        print(f"{'PASS' if ok else 'FAIL'} {name}: {detail}")
    return all(ok for _, ok, _ in results)


def args_of(argv):
    p = argparse.ArgumentParser()
    p.add_argument("out")
    p.add_argument("--trace")
    p.add_argument("--results", help="results JSONL (default <out>/results.jsonl)")
    p.add_argument("--labels", default="c1,main,newoff,newon")
    p.add_argument("--trace-label", default="newon")
    p.add_argument("--off-label", default="newoff")
    p.add_argument("--hash-labels", default="main,newoff,newon")
    p.add_argument("--contexts", default="8192,16384")
    p.add_argument("--threads", default="0,1,2")
    p.add_argument("--reps", type=int, default=3)
    p.add_argument("--rows", action="store_true")
    p.add_argument("--require-stream", action="store_true",
                   help="require qualified active streaming cancel for every matrix cell")
    a = p.parse_args(argv)
    a.labels, a.hash_labels = a.labels.split(","), a.hash_labels.split(",")
    a.contexts, a.threads = a.contexts.split(","), a.threads.split(",")
    return a


def check_args(a):
    """Settings that would make a rule vacuous are malformed input (review N7-R1 finding 3)."""
    if a.trace_label == a.off_label:
        raise BadInput("--trace-label and --off-label must differ")
    if a.reps < 1:
        raise BadInput("--reps must be >= 1")
    if not {a.trace_label, a.off_label} <= set(a.hash_labels):
        raise BadInput("--hash-labels must include the trace-on and trace-off labels")
    if not {a.trace_label, a.off_label, *a.hash_labels} <= set(a.labels):
        raise BadInput("--labels must include the trace, off and hash labels")
    if not all(a.contexts) or not all(a.threads):
        raise BadInput("--contexts/--threads must not be empty")


def main(argv):
    a = args_of(argv)
    try:
        check_args(a)
        out = pathlib.Path(a.out)
        rows = load_jsonl(pathlib.Path(a.results) if a.results else out / "results.jsonl", "results")
        validate(rows)
        joined = join(rows, load_traces(out, a.trace, rows), a.trace_label)
    except BadInput as e:
        print(f"MALFORMED INPUT: {e}")
        return 2
    if a.rows:
        for j in joined:
            print(json.dumps({k: j.get(k) for k in ("tag", "kind", "rep", "tailWallMs", "errorCode", "busyOk", "quiesced", "stages")}))
    describe(joined)
    passed = rules(rows, joined, a)
    print(f"\nVERDICT: {'PASS' if passed else 'FAIL'}")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
