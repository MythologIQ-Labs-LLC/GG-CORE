"""Isolated CLI conformance negatives, no real inference model required."""
import pathlib
import subprocess
import unittest
from unittest import mock

from cli_contract import CliContractError, check_public_cli

VERSION = "0.10.0-rc.1"


def fake_result(args, **kwargs):
    command = args[1]
    if command in ("version", "--version", "-V"):
        return subprocess.CompletedProcess(args, 0, f"GG-CORE {VERSION}\n", "")
    if command in ("help", "--help", "-h"):
        return subprocess.CompletedProcess(args, 0, "", "USAGE:\nCOMMANDS:\n")
    return subprocess.CompletedProcess(args, 1, "", "Unknown command: test\n")


class CliContractTests(unittest.TestCase):
    def run_contract(self, responder=fake_result):
        with mock.patch("cli_contract.subprocess.run", side_effect=responder) as runner:
            result = check_public_cli(pathlib.Path("gg-core-cli"), VERSION)
            self.assertEqual(runner.call_count, 7)
            for call in runner.call_args_list:
                self.assertNotIn("CORE_AUTH_TOKEN", call.kwargs["env"])
            return result

    def test_positive_receipt_is_explicitly_narrow(self):
        receipt = self.run_contract()
        self.assertEqual(receipt["result"], "PASS")
        self.assertFalse(receipt["ipc_qualified"])
        self.assertFalse(receipt["inference_qualified"])

    def test_wrong_version_refuses(self):
        def bad(args, **kwargs):
            result = fake_result(args, **kwargs)
            if args[1] == "--version":
                result.stdout = "GG-CORE mismatched\n"
            return result
        with mock.patch("cli_contract.subprocess.run", side_effect=bad):
            with self.assertRaisesRegex(CliContractError, "version alias"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_nonzero_version_exit_refuses(self):
        def bad(args, **kwargs):
            result = fake_result(args, **kwargs)
            if args[1] == "version":
                result.returncode = 9
            return result
        with mock.patch("cli_contract.subprocess.run", side_effect=bad):
            with self.assertRaisesRegex(CliContractError, "version alias"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_help_missing_usage_refuses(self):
        def bad(args, **kwargs):
            result = fake_result(args, **kwargs)
            if args[1] == "help":
                result.stderr = "empty help"
            return result
        with mock.patch("cli_contract.subprocess.run", side_effect=bad):
            with self.assertRaisesRegex(CliContractError, "help alias"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_help_stdout_contract_refuses(self):
        def bad(args, **kwargs):
            result = fake_result(args, **kwargs)
            if args[1] == "-h":
                result.stdout = "unexpected"
            return result
        with mock.patch("cli_contract.subprocess.run", side_effect=bad):
            with self.assertRaisesRegex(CliContractError, "help alias"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_unknown_command_success_refuses(self):
        def bad(args, **kwargs):
            result = fake_result(args, **kwargs)
            if args[1].startswith("__"):
                result.returncode = 0
            return result
        with mock.patch("cli_contract.subprocess.run", side_effect=bad):
            with self.assertRaisesRegex(CliContractError, "unknown command"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_unknown_command_no_diagnostic_refuses(self):
        def bad(args, **kwargs):
            result = fake_result(args, **kwargs)
            if args[1].startswith("__"):
                result.stderr = ""
            return result
        with mock.patch("cli_contract.subprocess.run", side_effect=bad):
            with self.assertRaisesRegex(CliContractError, "unknown command"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_timeout_refuses(self):
        with mock.patch("cli_contract.subprocess.run",
                        side_effect=subprocess.TimeoutExpired("test", 15)):
            with self.assertRaisesRegex(CliContractError, "invocation failed"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_executable_missing_refuses(self):
        with mock.patch("cli_contract.subprocess.run", side_effect=OSError("missing")):
            with self.assertRaisesRegex(CliContractError, "invocation failed"):
                check_public_cli(pathlib.Path("test"), VERSION)

    def test_blank_expected_version_refuses_without_execution(self):
        with mock.patch("cli_contract.subprocess.run") as runner:
            with self.assertRaisesRegex(CliContractError, "must be nonempty"):
                check_public_cli(pathlib.Path("test"), "")
            runner.assert_not_called()


if __name__ == "__main__":
    unittest.main()
