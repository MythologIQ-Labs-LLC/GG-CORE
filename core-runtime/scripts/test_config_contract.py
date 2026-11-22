"""Negative controls for public config CLI conformance (#114)."""
import pathlib
import subprocess
import unittest
from unittest import mock

from cli_contract import CliContractError
from config_contract import check_config_contract


def expected(args, **kwargs):
    mode = args[1]
    env = kwargs["env"]
    invalid = ("GG_CORE_MAX_CONCURRENT" if env.get("GG_CORE_MAX_CONCURRENT") == "0" else
               "GG_CORE_N_CTX" if env.get("GG_CORE_N_CTX") == "not-an-integer" else
               "GG_CORE_MAX_MEMORY_PER_CALL" if env.get("GG_CORE_MAX_MEMORY_PER_CALL") == "2097152" else None)
    if invalid:
        return subprocess.CompletedProcess(args, 1, "", f"ERROR: invalid {invalid}\n")
    if mode == "validate":
        return subprocess.CompletedProcess(args, 0, "Configuration is valid.\n", "")
    return subprocess.CompletedProcess(args, 0,
        f"GG_CORE_MAX_CONCURRENT={env.get('GG_CORE_MAX_CONCURRENT', '2')}\n", "")


class ConfigContractTests(unittest.TestCase):
    def check(self, override=None):
        calls = []
        def run(binary, args, env, cwd):
            self.assertNotIn("CORE_AUTH_TOKEN", env)
            self.assertEqual(env.get("GG_CORE_SOCKET_PATH"), None)
            calls.append((args, env))
            value = expected(args, env=env)
            return override(args, value, {"env": env}) if override else value
        with mock.patch("config_contract.invoke", side_effect=run):
            receipt = check_config_contract(pathlib.Path("gg-core-cli"))
        self.assertEqual(len(calls), 9)
        return receipt

    def test_complete_positive_contract_is_narrow(self):
        receipt = self.check()
        self.assertEqual(receipt["checked_commands"], 9)
        self.assertEqual(receipt["invalid_cases"], 3)
        self.assertFalse(receipt["ipc_qualified"])

    def test_invalid_zero_silently_accepted_refused(self):
        def bad(args, result, kw):
            if kw["env"].get("GG_CORE_MAX_CONCURRENT") == "0" and args[-1] == "validate":
                result.returncode = 0
            return result
        with self.assertRaisesRegex(CliContractError, "zero_concurrency"):
            self.check(bad)

    def test_invalid_context_no_diagnostic_refused(self):
        def bad(args, result, kw):
            if kw["env"].get("GG_CORE_N_CTX") == "not-an-integer":
                result.stderr = "unknown error"
            return result
        with self.assertRaisesRegex(CliContractError, "invalid_context"):
            self.check(bad)

    def test_memory_contradiction_show_leaking_values_refused(self):
        def bad(args, result, kw):
            if "GG_CORE_MAX_MEMORY_PER_CALL" in kw["env"] and args[-1] == "show":
                result.stdout = "GG_CORE_MAX_TOTAL_MEMORY=1048576\n"
            return result
        with self.assertRaisesRegex(CliContractError, "contradictory_memory"):
            self.check(bad)

    def test_default_validation_error_refused(self):
        def bad(args, result, kw):
            if "GG_CORE_MAX_CONCURRENT" not in kw["env"] and args[-1] == "validate":
                result.returncode = 1
            return result
        with self.assertRaisesRegex(CliContractError, "default config"):
            self.check(bad)

    def test_override_is_not_applied_refused(self):
        def bad(args, result, kw):
            if kw["env"].get("GG_CORE_MAX_CONCURRENT") == "3" and args[-1] == "show":
                result.stdout = "GG_CORE_MAX_CONCURRENT=2\n"
            return result
        with self.assertRaisesRegex(CliContractError, "override config show"):
            self.check(bad)

    def test_success_with_stderr_refused(self):
        def bad(args, result, kw):
            if kw["env"].get("GG_CORE_MAX_CONCURRENT") == "3":
                result.stderr = "warning: silently clamped"
            return result
        with self.assertRaisesRegex(CliContractError, "override"):
            self.check(bad)

    def test_malformed_failure_with_stdout_refused(self):
        def bad(args, result, kw):
            if kw["env"].get("GG_CORE_MAX_CONCURRENT") == "0":
                result.stdout = "Configuration is valid.\n"
            return result
        with self.assertRaisesRegex(CliContractError, "zero_concurrency"):
            self.check(bad)


if __name__ == "__main__":
    unittest.main()
