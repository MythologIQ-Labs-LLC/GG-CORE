"""Model-free negative controls for the native Windows CUDA execution receipt."""
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gguf_cuda_native_gate as gate


def synthetic(names, markers):
    return (
        "running " + str(len(names)) + " tests\n"
        + "".join(f"test {name} ... ok\n" for name in names)
        + "".join(f"{marker}\n" for marker in markers)
        + (f"test result: ok. {len(names)} passed; 0 failed; "
           "0 ignored; 0 measured; 0 filtered out;\n")
    )


class NativeReceiptNegativeControls(unittest.TestCase):
    def test_real_test_count_names_and_evidence_required(self):
        log = synthetic(gate.GPU_TESTS, gate.GPU_MARKERS)
        self.assertTrue(gate.executed_tests_pass(log, 0, gate.GPU_TESTS, gate.GPU_MARKERS))
        self.assertFalse(gate.executed_tests_pass(log, 101, gate.GPU_TESTS, gate.GPU_MARKERS))
        self.assertFalse(gate.executed_tests_pass(log.replace("6 passed", "5 passed"), 0,
                                                   gate.GPU_TESTS, gate.GPU_MARKERS))
        self.assertFalse(gate.executed_tests_pass(log.replace(
            "q3_gpu_stream_cancellation_stops_and_model_stays_usable ... ok",
            "q3_gpu_stream_cancellation_stops_and_model_stays_usable ... ignored"
        ), 0, gate.GPU_TESTS, gate.GPU_MARKERS))
        self.assertFalse(gate.executed_tests_pass(log.replace(
            "GPUQUAL cancel.stop_observed=true", "GPUQUAL cancel.stop_observed=false"
        ), 0, gate.GPU_TESTS, gate.GPU_MARKERS))

    def test_oom_requires_real_refusal_and_both_executed_tests(self):
        log = synthetic(gate.OOM_TESTS, gate.OOM_MARKERS)
        self.assertTrue(gate.executed_tests_pass(log, 0, gate.OOM_TESTS, gate.OOM_MARKERS))
        self.assertFalse(gate.executed_tests_pass(log.replace(
            "GPUQUAL oom.refusal=PASS", "GPUQUAL oom.refusal=FAIL"
        ), 0, gate.OOM_TESTS, gate.OOM_MARKERS))
        self.assertFalse(gate.executed_tests_pass(log.replace("2 passed", "1 passed"), 0,
                                                   gate.OOM_TESTS, gate.OOM_MARKERS))

    def test_hardware_identity_and_capacity_must_match(self):
        uuid = "GPU-12345678-abcd-9876-def0-123456789abc"
        valid = f"{uuid}, NVIDIA RTX 5070 Ti, 16303, 590.01"
        self.assertEqual(gate.pinned_gpu_information(valid, uuid, 16303)["memory_mib"], 16303)
        for line, expected in (
            (valid, 16304),
            (valid + "\n" + valid, 16303),
            (valid.replace(uuid, "GPU-87654321-abcd-9876-def0-123456789abc"), 16303),
            (valid.replace("16303", "32768"), 32768),
            (valid.replace("16303", "unknown"), 16303),
            (valid.replace("590.01", ""), 16303),
        ):
            with self.subTest(line=line, expected=expected):
                with self.assertRaises(RuntimeError):
                    gate.pinned_gpu_information(line, uuid, expected)

    def test_markers_without_executed_cases_do_not_pass(self):
        self.assertFalse(gate.executed_tests_pass(
            "\n".join(gate.GPU_MARKERS), 0, gate.GPU_TESTS, gate.GPU_MARKERS
        ))


if __name__ == "__main__":
    unittest.main()
