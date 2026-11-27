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


    @staticmethod
    def stream_rows():
        return [
            {"kind": "streamCancel", "tag": tag, "rep": 0, "requestId": 990 + j,
             "started": True, "prefixFrames": 1, "firstTextMs": 420,
             "ack": True, "ackCount": 1, "ackMs": 8,
             "terminalCount": 1, "terminalMs": 31, "terminalBeforeAck": False,
             "terminalError": "stream cancelled", "dataAfterAck": 0,
             "dataAfterFinal": 0, "lateFrames": 0, "unexpectedFrames": 0,
             "readOutcome": "OK", "postTerminalRead": "TIMEOUT",
             "postTerminalProbePhase": "afterQuiescence", "quiesced": True,
             "tailWallMs": 440, "tailBound": "conservativeUpperBound",
             "cpuObservationStartMs": 380, "cpuObservationStartUnixMs": 100380,
             "stopSentUnixMs": 100000, "tailClockOriginUnixMs": 100000}
            for j, tag in enumerate((TAG_OFF, TAG_ON))
        ]

    def test_r8_active_stream_receipts_pass_only_when_required(self):
        rows, events = evidence()
        code, detail = run(rows + self.stream_rows(), events, ["--require-stream"])
        self.assertEqual(code, 0, detail)
        self.assertIn("PASS R8", detail)
        code, detail = run(rows, events)
        self.assertEqual(code, 0, detail)  # historical R1-R7 are unaffected

    def test_r8_old_before_quiescence_probe_cannot_claim_no_late_frames(self):
        rows, events = evidence()
        stream = self.stream_rows()
        # Even a TIMEOUT is not proof if the worker was still in native work.
        stream[0]["postTerminalProbePhase"] = "beforeQuiescence"
        code, detail = run(rows + stream, events, ["--require-stream"])
        self.assertEqual(code, 1, detail)
        self.assertIn("FAIL R8", detail)

        # Historical evidence with no probe phase is also unqualified.
        stream[0].pop("postTerminalProbePhase")
        code, detail = run(rows + stream, events, ["--require-stream"])
        self.assertEqual(code, 1, detail)
        self.assertIn("FAIL R8", detail)

    def test_stream_harness_probes_after_cpu_quiescence(self):
        # Structural source check, not a substitute for running the VM.
        script = (HERE / "n7_packaged_stop_tail.ps1").read_text(encoding="utf-8")
        start = script.index("function Stream-Cancel {")
        end = script.index("# Stage every daemon first", start)
        stream = script[start:end]
        self.assertLess(
            stream.index("$q = Quiesce $t0 $cpu0"),
            stream.index("$post = Probe-Late-Bytes $c 350"),
        )
        self.assertIn("postTerminalProbePhase = $probePhase", stream)
        self.assertNotIn("Recv-Ipc $c 350", stream)
        self.assertIn("$read = $c.ReadAsync($buf, 0, 1)", script)
        self.assertIn("if ($post -ne 'TIMEOUT') { $late++ }", stream)

    def test_r8_post_terminal_single_byte_is_a_failure(self):
        rows, events = evidence()
        stream = self.stream_rows()
        # Even if an outer client did not count a complete late frame,
        # a post-terminal byte (including partial IPC length prefix) is invalid.
        stream[0]["postTerminalRead"] = "LATE_BYTES"
        code, detail = run(rows + stream, events, ["--require-stream"])
        self.assertEqual(code, 1, detail)
        self.assertIn("FAIL R8", detail)

    def test_r8_missing_active_stream_fails(self):
        rows, events = evidence()
        code, detail = run(rows, events, ["--require-stream"])
        self.assertEqual(code, 1, detail)
        self.assertIn("FAIL R8", detail)

    def test_r8_early_ended_stream_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["started"] = False
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

    def test_r8_late_stream_frame_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["lateFrames"] = 1
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

    def test_r8_data_after_ack_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["dataAfterAck"] = 1
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

    def test_r8_terminal_before_ack_is_permitted(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["terminalBeforeAck"] = True
        stream[0]["terminalMs"] = 4
        stream[0]["ackMs"] = 8
        code, detail = run(rows + stream, events, ["--require-stream"])
        self.assertEqual(code, 0, detail)

    def test_r8_data_after_terminal_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["dataAfterFinal"] = 1
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

    def test_r8_zero_tail_before_delayed_cpu_observation_is_refused(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["tailWallMs"] = 0
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)
        stream[0]["tailWallMs"] = 440
        stream[0]["tailBound"] = "exact"
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

    def test_r8_terminal_missing_or_success_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["terminalCount"] = 0
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)
        stream[0]["terminalCount"] = 1
        stream[0]["terminalError"] = ""
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

    def test_r8_missing_or_duplicate_matrix_cell_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        self.assertEqual(run(rows + stream[:1], events, ["--require-stream"])[0], 1)
        self.assertEqual(run(rows + stream + stream[:1], events, ["--require-stream"])[0], 2)

    def test_r8_ambiguous_cpu_measurement_fails(self):
        rows, events = evidence()
        stream = self.stream_rows()
        stream[0]["quiesced"] = False
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)
        stream[0]["quiesced"] = True
        stream[0]["tailClockOriginUnixMs"] = 100300
        self.assertEqual(run(rows + stream, events, ["--require-stream"])[0], 1)

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
        old = next(r for r in rows if r["kind"] == "prefillCancel" and r["tag"] == TAG_OFF)
        rows.remove(old)
        rows.extend([{**old, "rep": 1, "requestId": 99001},
                     {**old, "rep": 1, "requestId": 99002}])
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

    def test_zero_throughput_duration_is_malformed(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_OFF)["ms"] = 0
        self.assertEqual(run(rows, events)[0], 2)

    def test_negative_throughput_duration_is_malformed(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_OFF)["ms"] = -1
        self.assertEqual(run(rows, events)[0], 2)

    def test_nonfinite_throughput_duration_is_malformed(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput" and r["tag"] == TAG_OFF)["ms"] = float("nan")
        self.assertEqual(run(rows, events)[0], 2)

    def test_nonfinite_trace_time_is_malformed(self):
        rows, events = evidence()
        events[2]["t"] = float("inf")
        self.assertEqual(run(rows, events)[0], 2)

    # Deadline anchoring (2026-11-27, combined-candidate N7 R1 "stage order"
    # on prefillDeadline): the answering task and the backend observe the
    # same deadline instant independently, so S3 may precede S2 by the
    # answer's reported lag; it may never precede the deadline itself.
    @staticmethod
    def deadline_stages(events):
        s2 = next(e for e in events if e["message"] == "stop answered to caller"
                  and e["kind"] == "deadline_exceeded")
        s3 = next(e for e in events if e["message"] == "backend stop observed"
                  and e["queue_id"] == s2["queue_id"])
        return s2, s3

    def test_r1_deadline_s3_before_s2_within_reported_lag_is_complete(self):
        rows, events = evidence()
        s2, s3 = self.deadline_stages(events)
        s2["deadline_lag_ms"] = 12.0
        s3["t"] = s2["t"] - 0.007  # observed 7 ms before the late answer, 5 ms after the deadline
        code, text = run(rows, events)
        self.assertEqual(code, 0, text)
        self.assertIn("VERDICT: PASS", text)

    def test_r1_deadline_s3_before_the_deadline_instant_fails(self):
        rows, events = evidence()
        s2, s3 = self.deadline_stages(events)
        s2["deadline_lag_ms"] = 12.0
        s3["t"] = s2["t"] - 0.020  # 8 ms before the deadline: impossible chronology
        self.assert_fails("R1", rows, events)

    def test_r1_deadline_without_lag_keeps_the_legacy_answer_anchor(self):
        rows, events = evidence()
        s2, s3 = self.deadline_stages(events)
        s3["t"] = s2["t"] - 0.007  # an old daemon cannot prove this was after the deadline
        self.assert_fails("R1", rows, events)

    def test_r1_cancel_s3_before_s2_fails_even_with_a_lag_field(self):
        rows, events = evidence()
        s2 = next(e for e in events if e["message"] == "stop answered to caller" and e["kind"] == "cancelled")
        s3 = next(e for e in events if e["message"] == "backend stop observed" and e["queue_id"] == s2["queue_id"])
        s2["deadline_lag_ms"] = 12.0  # meaningless on a cancel; the answer stays the anchor
        s3["t"] = s2["t"] - 0.007
        self.assert_fails("R1", rows, events)

    def test_r5_deadline_observation_is_measured_from_the_deadline_instant(self):
        rows, events = evidence()
        s2, s3 = self.deadline_stages(events)
        s2["deadline_lag_ms"] = 300.0
        s3["last_decode_ms"] = 10.0
        s3["t"] = s2["t"] - 0.280  # 20 ms after the deadline: beyond 10 + 5 ms from the anchor
        self.assert_fails("R5", rows, events)

    def test_negative_deadline_lag_is_malformed(self):
        rows, events = evidence()
        s2, _ = self.deadline_stages(events)
        s2["deadline_lag_ms"] = -1.0
        self.assertEqual(run(rows, events)[0], 2)

    def test_r6_contention_samples_are_reported_not_judged(self):
        rows, events = evidence()
        for r in rows:
            if r["kind"] == "throughput":
                r["guestOtherCpuPct"] = 35.5
                r["daemonCpuPct"] = 12.0
                r["hostLoad"] = "2026-11-27T00:00:00Z cpu=80%"
        code, text = run(rows, events)
        self.assertEqual(code, 0, text)
        self.assertIn("guestOtherCpu max 35.5%", text)
        self.assertIn("pairs 1.000", text)
        self.assertIn("cpu=80%", text)

    def test_malformed_contention_sample_is_refused(self):
        rows, events = evidence()
        next(r for r in rows if r["kind"] == "throughput")["guestOtherCpuPct"] = "high"
        self.assertEqual(run(rows, events)[0], 2)

    def test_nonfinite_native_decode_duration_is_malformed(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "backend stop observed")["last_decode_ms"] = float("nan")
        self.assertEqual(run(rows, events)[0], 2)

    def test_negative_native_decode_duration_is_malformed(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "backend stop observed")["last_decode_ms"] = -1
        self.assertEqual(run(rows, events)[0], 2)

    def test_non_integer_queue_id_is_malformed(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "backend stop observed")["queue_id"] = "1"
        self.assertEqual(run(rows, events)[0], 2)

    def test_invalid_dropped_count_is_malformed(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "stop answered to caller")["dropped"] = -1
        self.assertEqual(run(rows, events)[0], 2)

    def test_untyped_matched_flag_is_malformed(self):
        rows, events = evidence()
        next(e for e in events if e["message"] == "cancel request received")["matched"] = "true"
        self.assertEqual(run(rows, events)[0], 2)

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

    def committed_gate(self, name, labels):
        """Run the actual committed measurement through the gate, unchanged."""
        docs = HERE.parents[1] / "docs" / "integration"
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = gate.main([str(docs), "--results", str(docs / f"N7_STOP_STAGE_RESULTS_{name}.jsonl"),
                              "--trace", str(docs / f"N7_STOP_STAGE_TRACE_{name}.jsonl"),
                              "--labels", labels])
        return code, out.getvalue()

    def test_committed_fcfa3cc_evidence_passes(self):
        code, text = self.committed_gate("n7stage", "c1,main,newoff,newon")
        self.assertEqual(code, 0, text)
        self.assertIn("VERDICT: PASS", text)

    def test_committed_n7m2_integrated_evidence_passes(self):
        code, text = self.committed_gate("n7m2", "main,newoff,newon")
        self.assertEqual(code, 0, text)
        self.assertIn("VERDICT: PASS", text)

    def test_committed_n7m1_failed_evidence_stays_failed(self):
        code, text = self.committed_gate("n7m1", "main,newoff,newon")
        self.assertEqual(code, 1, text)
        self.assertIn("VERDICT: FAIL", text)
        self.assertIn("FAIL R3", text)


if __name__ == "__main__":
    unittest.main()
