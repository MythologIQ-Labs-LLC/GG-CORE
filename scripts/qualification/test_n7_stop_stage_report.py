"""Tests for the fail-closed #184 evidence gate (review N7-R1).

A synthetic, minimal but complete evidence set passes (exit 0); each
acceptance rule is then broken on its own and must fail (exit 1); malformed
or missing input must exit 2. The committed fcfa3cc evidence must pass.

Run: python -I -m unittest discover -s scripts/qualification -p "test_n7_*.py"
"""

import contextlib
import io
import json
import pathlib
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import n7_stop_stage_report as gate  # noqa: E402

ARGS = ["--labels", "newoff,newon", "--hash-labels", "newoff,newon",
        "--contexts", "8192", "--threads", "1", "--reps", "1"]
TAG_ON, TAG_OFF = "newon-ctx8192-t1", "newoff-ctx8192-t1"


def evidence():
    """A complete, passing set: rows plus the trace-on daemon's events."""
    rows, events, rid, qid, t = [], [], 100, 0, 0.0
    events.append({"tag": TAG_ON, "message": "stop trace started", "unix_ms": 1_000_000, "dropped": 0, "t": 0.0})
    for tag in (TAG_OFF, TAG_ON):
        rows.append({"kind": "throughput", "tag": tag, "rep": 0, "ms": 1000, "ok": True, "outputSha": "AA"})
        for kind in gate.STOPS:
            rid, qid, t = rid + 1, qid + 1, t + 10
            row = {"kind": kind, "tag": tag, "rep": 0, "requestId": rid, "errorCode": gate.CODES[kind],
                   "frames": gate.FRAMES, "quiesced": True, "tailWallMs": 300, "stopSentUnixMs": 1_000_000 + t * 1000}
            if kind == "queuedCancel":
                row.update(busyOk=2, quiesced=None, tailWallMs=None)
            rows.append(row)
            if tag == TAG_ON:
                events += stop_events(kind, rid, qid, t)
        rows.append({"kind": "recover", "tag": tag, "rep": 0, "ok": True, "alive": True})
    return rows, events


def stop_events(kind, rid, qid, t):
    ev = lambda m, dt, **f: {"tag": TAG_ON, "message": m, "dropped": 0, "t": t + dt, **f}
    out = []
    if kind != "prefillDeadline":
        out.append(ev("cancel request received", 0.0, request_id=rid, matched=True))
    out.append(ev("stop answered to caller", 0.0001, request_id=rid, queue_id=qid,
                  kind=gate.CODES[kind], stream=False))
    if kind != "queuedCancel":
        out.append(ev("backend stop observed", 0.2, queue_id=qid, stream=False, phase="prefill",
                      last_decode_ms=400.0, last_decode_tokens=64))
        out.append(ev("worker released after stop", 0.21, queue_id=qid, stream=False, cancel_flag=True))
    return out


def run(rows, events, extra=(), raw_results=None):
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)
        (d / "results.jsonl").write_text(raw_results if raw_results is not None
                                         else "\n".join(json.dumps(r) for r in rows), encoding="utf-8")
        (d / "trace.jsonl").write_text("\n".join(json.dumps(e) for e in events), encoding="utf-8")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = gate.main([str(d), "--trace", str(d / "trace.jsonl"), *ARGS, *extra])
        return code, out.getvalue()


def failed_rules(text):
    return {line.split()[1] for line in text.splitlines() if line.startswith("FAIL ")}


class Gate(unittest.TestCase):
    def assert_fails(self, rule, rows, events):
        code, text = run(rows, events)
        self.assertEqual(code, 1, text)
        self.assertIn(rule, failed_rules(text), text)
        self.assertIn("VERDICT: FAIL", text)

    def test_complete_evidence_passes(self):
        code, text = run(*evidence())
        self.assertEqual(code, 0, text)
        self.assertEqual(failed_rules(text), set())

    def test_r1_dropped_trace_line_fails(self):
        rows, events = evidence()
        events[-1]["dropped"] = 1
        self.assert_fails("R1", rows, events)

    def test_r1_missing_release_stage_fails(self):
        rows, events = evidence()
        events = [e for e in events if not (e["message"] == "worker released after stop" and e["queue_id"] == 5)]
        self.assert_fails("R1", rows, events)

    def test_r1_missing_answer_stage_fails(self):
        rows, events = evidence()
        events = [e for e in events if not (e["message"] == "stop answered to caller" and e["request_id"] == 106)]
        self.assert_fails("R1", rows, events)

    def test_incomplete_trace_is_not_counted_as_qualified(self):
        rows, events = evidence()
        events = [e for e in events if not (e["message"] == "worker released after stop" and e["queue_id"] == 5)]
        _, text = run(rows, events)
        self.assertIn("R5", failed_rules(text), "the incomplete stop's cell has no qualified timed stop")
        self.assertIn("1 incomplete of 4 traced", text)

    def test_r2_queued_cancel_reaching_backend_fails(self):
        rows, events = evidence()
        events.append({"tag": TAG_ON, "message": "backend stop observed", "queue_id": 8, "stream": False,
                       "dropped": 0, "t": 80.2, "phase": "prefill", "last_decode_ms": 1.0, "last_decode_tokens": 64})
        self.assert_fails("R2", rows, events)

    def test_r3_missing_matrix_cell_fails(self):
        rows, events = evidence()
        rows = [r for r in rows if not (r["kind"] == "decodeCancel" and r["tag"] == TAG_OFF)]
        self.assert_fails("R3", rows, events)

    def test_r3_missing_configuration_fails(self):
        rows, events = evidence()
        code, text = run(rows, events, extra=["--threads", "1,2"])
        self.assertEqual(code, 1, text)
        self.assertIn("R3", failed_rules(text))

    def test_r3_missing_recovery_fails(self):
        rows, events = evidence()
        rows = [r for r in rows if r["kind"] != "recover" or r["tag"] != TAG_ON]
        self.assert_fails("R3", rows, events)

    def test_r3_surplus_recovery_cannot_hide_a_missing_one(self):
        rows, events = evidence()
        off = next(r for r in rows if r["kind"] == "recover" and r["tag"] == TAG_OFF)
        rows = [r for r in rows if r["kind"] != "recover" or r["tag"] != TAG_ON] + [{**off, "rep": 1}]
        self.assert_fails("R3", rows, events)

    def test_r1_explicit_cancel_without_arrival_stage_fails(self):
        rows, events = evidence()
        events = [e for e in events if e["message"] != "cancel request received"]
        self.assert_fails("R1", rows, events)

    def test_r1_deadline_needs_no_arrival_stage(self):
        rows, events = evidence()
        self.assertFalse(any(e["message"] == "cancel request received" and e.get("request_id") == 106
                             for e in events), "fixture: the deadline stop has no S1")
        code, text = run(rows, events)
        self.assertEqual(code, 0, text)

    # Adversarial matrix identity: counts without repetition identities are insufficient.
    def test_r3_duplicate_stop_rep_hides_missing_rep_fails(self):
        rows, events = evidence()
        rows.append({**next(r for r in rows if r["kind"] == "prefillCancel" and r["tag"] == TAG_OFF),
                     "requestId": 99001})
        self.assert_fails("R3", rows, events)

    def test_r3_out_of_range_stop_rep_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "prefillCancel" and r["tag"] == TAG_OFF)["rep"] = 9
        self.assert_fails("R3", rows, events)

    def test_r3_duplicate_throughput_rep_fails(self):
        rows, events = evidence()
        rows.append(dict(next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_OFF)))
        self.assert_fails("R3", rows, events)

    def test_malformed_boolean_rep_is_refused(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "recover")["rep"] = True
        self.assertEqual(run(rows, events)[0], 2)

    def test_malformed_negative_rep_is_refused(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput")["rep"] = -1
        self.assertEqual(run(rows, events)[0], 2)

    def test_r3_duplicate_recovery_rep_fails(self):
        rows, events = evidence()
        rows.append(dict(next(r for r in rows if r["kind"] == "recover" and r["tag"] == TAG_OFF)))
        self.assert_fails("R3", rows, events)

    def test_r1_backend_observation_before_answer_fails(self):
        rows, events = evidence()
        s2 = next(e for e in events if e["message"] == "stop answered to caller")
        s3 = next(e for e in events if e["message"] == "backend stop observed")
        s3["t"] = s2["t"] - 0.1
        self.assert_fails("R1", rows, events)

    def test_r1_worker_release_before_backend_observation_fails(self):
        rows, events = evidence()
        s3 = next(e for e in events if e["message"] == "backend stop observed")
        s4 = next(e for e in events if e["message"] == "worker released after stop")
        s4["t"] = s3["t"] - 0.01
        self.assert_fails("R1", rows, events)

    def test_r1_cancel_arrival_after_answer_fails(self):
        rows, events = evidence()
        s1 = next(e for e in events if e["message"] == "cancel request received")
        s2 = next(e for e in events if e["message"] == "stop answered to caller")
        s1["t"] = s2["t"] + 0.01
        self.assert_fails("R1", rows, events)

    def test_r1_unmatched_cancel_arrival_fails(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "cancel request received")["matched"] = False
        self.assert_fails("R1", rows, events)

    def test_r1_wrong_answer_kind_fails(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "stop answered to caller")["kind"] = "deadline_exceeded"
        self.assert_fails("R1", rows, events)

    def test_r4_wrong_error_code_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "prefillDeadline")["errorCode"] = "cancelled"
        self.assert_fails("R4", rows, events)

    def test_r4_late_frame_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "decodeCancel")["frames"] = gate.FRAMES + ",stream_chunk"
        self.assert_fails("R4", rows, events)

    def test_r4_not_quiesced_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "prefillCancel")["quiesced"] = False
        self.assert_fails("R4", rows, events)

    def test_r4_busy_request_disturbed_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "queuedCancel")["busyOk"] = 1
        self.assert_fails("R4", rows, events)

    def test_r4_failed_recovery_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "recover")["alive"] = False
        self.assert_fails("R4", rows, events)

    def test_r5_observation_after_the_in_flight_call_fails(self):
        rows, events = evidence()
        for e in events:
            if e["message"] == "backend stop observed":
                e["last_decode_ms"] = 10.0  # observed 200 ms after S2, far beyond 10 + 5 ms
        self.assert_fails("R5", rows, events)

    def test_r5_below_95_percent_fails(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "backend stop observed")["last_decode_ms"] = 1.0
        self.assert_fails("R5", rows, events)  # 2/3 within < 95%

    def test_r5_no_timed_observation_fails(self):
        rows, events = evidence()
        for e in events:
            if e["message"] == "backend stop observed":
                e.pop("last_decode_ms")
        self.assert_fails("R5", rows, events)

    def test_r6_trace_overhead_above_3_percent_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_ON)["ms"] = 1031
        self.assert_fails("R6", rows, events)

    def test_r6_trace_speedup_beyond_3_percent_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_ON)["ms"] = 969
        self.assert_fails("R6", rows, events)

    def test_r6_exactly_3_percent_passes(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_ON)["ms"] = 1030
        code, text = run(rows, events)
        self.assertEqual(code, 0, text)

    def test_r6_missing_throughput_fails(self):
        rows, events = evidence()
        rows = [r for r in rows if not (r["kind"] == "throughput" and r["tag"] == TAG_OFF)]
        self.assert_fails("R6", rows, events)

    def test_r7_divergent_output_hash_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_ON)["outputSha"] = "BB"
        self.assert_fails("R7", rows, events)

    def test_missing_results_is_malformed(self):
        with tempfile.TemporaryDirectory() as d:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main([d, *ARGS]), 2)

    def test_non_json_results_line_is_malformed(self):
        rows, events = evidence()
        raw = "\n".join(json.dumps(r) for r in rows) + "\n{not json"
        self.assertEqual(run(rows, events, raw_results=raw)[0], 2)

    def test_empty_results_is_malformed(self):
        self.assertEqual(run([], [], raw_results="")[0], 2)

    def test_stop_row_without_request_id_is_malformed(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "prefillCancel").pop("requestId")
        self.assertEqual(run(rows, events)[0], 2)

    def test_bad_tag_is_malformed(self):
        rows, events = evidence()
        rows[0]["tag"] = "newon"
        self.assertEqual(run(rows, events)[0], 2)

    def test_trace_event_without_timestamp_is_malformed(self):
        rows, events = evidence()
        events[1].pop("t")
        self.assertEqual(run(rows, events)[0], 2)

    # Re-review findings (2026-11-15): each previously passing bad input now fails.
    def test_running_stop_without_backend_observation_fails(self):
        rows, events = evidence()
        events = [e for e in events if not (e["message"] == "backend stop observed" and e["queue_id"] == 5)]
        code, text = run(rows, events)
        self.assertEqual(code, 1, text)
        self.assertTrue({"R1", "R5"} <= failed_rules(text), text)

    def test_setup_phase_stop_is_checked_not_skipped(self):
        rows, events = evidence()
        for e in events:
            if e["message"] == "backend stop observed":
                e.pop("last_decode_ms")  # setup: nothing in flight, observed 200 ms after S2
                e["phase"] = "setup"
        self.assert_fails("R5", rows, events)

    def test_duplicate_request_id_is_malformed(self):
        rows, events = evidence()
        dup = dict(next(r for r in rows if r["kind"] == "decodeCancel" and r["tag"] == TAG_ON))
        dup["kind"] = "prefillDeadline"
        rows.append(dup)
        self.assertEqual(run(rows, events)[0], 2)

    def test_duplicate_answer_event_is_incomplete(self):
        rows, events = evidence()
        events.append(dict(next(e for e in events if e["message"] == "stop answered to caller")))
        self.assert_fails("R1", rows, events)

    def test_same_trace_and_off_label_is_malformed(self):
        rows, events = evidence()
        self.assertEqual(run(rows, events, extra=["--off-label", "newon"])[0], 2)

    def test_hash_labels_missing_on_off_labels_is_malformed(self):
        rows, events = evidence()
        self.assertEqual(run(rows, events, extra=["--hash-labels", "newon"])[0], 2)

    def test_zero_reps_is_malformed(self):
        rows, events = evidence()
        self.assertEqual(run(rows, events, extra=["--reps", "0"])[0], 2)

    def test_hash_label_not_in_labels_is_malformed(self):
        rows, events = evidence()
        self.assertEqual(run(rows, events, extra=["--hash-labels", "newoff,newon,main"])[0], 2)

    def test_missing_hash_label_throughput_fails(self):
        rows, events = evidence()
        rows = [r for r in rows if not (r["kind"] == "throughput" and r["tag"] == TAG_OFF)]
        code, text = run(rows, events)
        self.assertEqual(code, 1, text)
        self.assertTrue({"R6", "R7"} <= failed_rules(text), text)

    def test_throughput_without_output_hash_fails(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_OFF)["outputSha"] = None
        self.assert_fails("R7", rows, events)

    def test_non_numeric_trace_time_is_malformed(self):
        rows, events = evidence()
        events[2]["t"] = "soon"
        self.assertEqual(run(rows, events)[0], 2)

    def test_err_line_without_timestamp_is_malformed(self):
        with tempfile.TemporaryDirectory() as d:
            d = pathlib.Path(d)
            rows, _ = evidence()
            (d / "results.jsonl").write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
            (d / f"{TAG_ON}.err").write_text(json.dumps({"target": "gg_core::stop", "fields": {"message": "x"}}), encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main([str(d), *ARGS]), 2)

    def test_committed_fcfa3cc_evidence_passes(self):
        docs = HERE.parents[1] / "docs" / "integration"
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = gate.main([str(docs), "--results", str(docs / "N7_STOP_STAGE_RESULTS_n7stage.jsonl"),
                              "--trace", str(docs / "N7_STOP_STAGE_TRACE_n7stage.jsonl")])
        self.assertEqual(code, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
