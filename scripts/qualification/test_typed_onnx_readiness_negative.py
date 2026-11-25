"""#171: readiness failure reporting and missing-auth negatives. No daemon."""
import json
import subprocess
import unittest

import typed_onnx_packaged_acceptance as a
from typed_onnx_negative import NegativeError, unauthenticated_refused
from typed_onnx_readiness import typed_models_ready, wait_typed_ready

READY = [{"handle_id": 1, "state": "ready", "capabilities": ["embedding"]},
         {"handle_id": 2, "state": "ready", "capabilities": ["text_classification"]}]
NO_CAPS = [{k: v for k, v in m.items() if k != "capabilities"} for m in READY]


def done(code=0, payload=None):
    out = b"" if payload is None else json.dumps({"models": payload}).encode()
    return subprocess.CompletedProcess([], code, out, b"")


class Daemon:
    def __init__(self, exit_code=None):
        self.exit_code = exit_code

    def poll(self):
        return self.exit_code


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


def wait(listing, daemon=None, timeout=3):
    clock = Clock()
    return wait_typed_ready(lambda *a, **k: listing, "bin", {}, daemon or Daemon(),
                            timeout, sleep=clock.sleep, clock=clock)


class TestReadinessReporting(unittest.TestCase):
    def test_absent_capabilities_are_named_not_treated_as_ready(self):
        with self.assertRaisesRegex(ValueError, "ready-model-capabilities-absent"):
            typed_models_ready(json.dumps({"models": NO_CAPS}).encode())

    def test_timeout_reports_the_last_readiness_reason(self):
        self.assertEqual(wait(done(0, NO_CAPS)),
                         "readiness-timeout-ready-model-capabilities-absent")
        self.assertEqual(wait(done(0, READY[:1])), "readiness-timeout-typed-models-not-ready")
        self.assertEqual(wait(done(3)), "readiness-timeout-models-list-refused")
        self.assertEqual(wait(done(0, READY), timeout=0), "daemon-startup-timeout")

    def test_ready_listing_and_early_exit(self):
        self.assertIsNone(wait(done(0, READY)))
        self.assertEqual(wait(done(0, READY), Daemon(1)), "daemon-exited-before-ready")

    def test_hung_listing_probe_is_retried_not_fatal(self):
        def hung(*args, **kwargs):
            raise a.AcceptanceError("command-timeout")
        original, a.invoke = a.invoke, hung
        try:
            self.assertEqual(a._invoke_probe("bin", {}, ["models"]).returncode, 1)
        finally:
            a.invoke = original


class TestMissingAuth(unittest.TestCase):
    def run_case(self, listing, typed):
        seen = []

        def invoke(binary, env, args, timeout):
            seen.append("CORE_AUTH_TOKEN" in env)
            return listing if args[0] == "models" else typed
        unauthenticated_refused(invoke, "bin", {"CORE_AUTH_TOKEN": "t", "X": "1"}, "m")
        self.assertEqual(seen, [False, False], "token must be absent on both calls")

    def test_refused_or_capability_free_listing_passes(self):
        self.run_case(done(0, NO_CAPS), done(1))
        self.run_case(done(1), done(1))

    def test_exposed_capabilities_or_allowed_inference_fail(self):
        with self.assertRaisesRegex(NegativeError, "unauthenticated-capabilities-exposed"):
            self.run_case(done(0, READY), done(1))
        with self.assertRaisesRegex(NegativeError, "unauthenticated-typed-inference-allowed"):
            self.run_case(done(0, NO_CAPS), done(0))
        with self.assertRaisesRegex(NegativeError, "unauthenticated-listing-invalid"):
            self.run_case(subprocess.CompletedProcess([], 0, b"nope", b""), done(1))


if __name__ == "__main__":
    unittest.main()
