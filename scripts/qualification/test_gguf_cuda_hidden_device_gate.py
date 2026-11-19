"""Model-free negative controls for the CUDA hidden-device evidence parser."""
import importlib.util
from pathlib import Path
import unittest

MODULE = Path(__file__).with_name("gguf_cuda_hidden_device_gate.py")
SPEC = importlib.util.spec_from_file_location("cuda_hidden_device_gate", MODULE)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

ONE_PASS = (
    "running 1 test\n"
    "test cuda_build_without_a_visible_device_refuses_offload_but_serves_cpu ... ok\n"
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
)


class EvidencePassTests(unittest.TestCase):
    def test_real_marker_and_one_executed_test(self):
        self.assertTrue(gate.evidence_pass(gate.PASS_MARKER + "\n" + ONE_PASS, 0))

    def test_successful_exit_without_marker_is_not_qualified(self):
        self.assertFalse(gate.evidence_pass(ONE_PASS, 0))

    def test_conditional_skip_cannot_be_counted_as_pass(self):
        skipped = (
            "GPUQUAL hidden_device.qual_status=NOT_QUALIFIED (opt-in missing)\n"
            + ONE_PASS
        )
        self.assertFalse(gate.evidence_pass(skipped, 0))

    def test_no_executed_test_cannot_pass(self):
        log = (
            gate.PASS_MARKER + "\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 1 filtered out;\n"
        )
        self.assertFalse(gate.evidence_pass(log, 0))

    def test_failure_or_abort_cannot_pass(self):
        self.assertFalse(gate.evidence_pass(gate.PASS_MARKER + "\n" + ONE_PASS, 101))
        self.assertFalse(gate.evidence_pass(gate.PASS_MARKER + "\n" + ONE_PASS, 127))

    def test_unexpected_extra_ignored_test_does_not_pass(self):
        log = (
            gate.PASS_MARKER + "\n"
            "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured;\n"
        )
        self.assertFalse(gate.evidence_pass(log, 0))


if __name__ == "__main__":
    unittest.main()
