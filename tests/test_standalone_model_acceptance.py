"""Offline harness unit tests: no GG-CORE binary, model, or network required."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "standalone_model_acceptance.py"
spec = importlib.util.spec_from_file_location("standalone_model_acceptance", SCRIPT)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ModelAcceptanceHarnessTests(unittest.TestCase):
    def test_version_checks_source_and_feature(self):
        sha = "a" * 40
        doc = {"name": "GG-CORE", "commit": sha, "version": "1.0.0",
               "features": ["gguf", "onnx"], "protocol_version": "V1", "target": "windows-x86_64"}
        self.assertEqual(m.verify_version(doc, sha, "generation")["commit"], sha)
        self.assertEqual(m.verify_version(doc, sha, "embedding")["commit"], sha)
        with self.assertRaises(m.AcceptanceFailure):
            m.verify_version(doc, "b" * 40, "generation")
        with self.assertRaises(m.AcceptanceFailure):
            m.verify_version({**doc, "features": ["gguf"]}, sha, "classification")
        with self.assertRaises(m.AcceptanceFailure):
            m.verify_version({**doc, "commit": "unknown"}, sha, "generation")

    def test_typed_embedding_shape_and_numeric_safety(self):
        ok = {"kind": "embedding", "dimensions": 3, "vector": [0.1, 0.2, 0.3]}
        self.assertEqual(m.validate_typed(ok, "embedding"), {"kind": "embedding", "dimensions": 3})
        for bad in [
            {**ok, "dimensions": 4}, {**ok, "vector": [0.1, float("nan"), 0.3]},
            {**ok, "dimensions": True}, {**ok, "kind": "classification"}
        ]:
            with self.assertRaises(m.AcceptanceFailure):
                m.validate_typed(bad, "embedding")

    def test_classification_label_and_confidence(self):
        good = {"kind": "classification", "label": "positive", "confidence": 0.6,
                "all_labels": [{"label": "positive", "confidence": 0.6}]}
        self.assertEqual(m.validate_typed(good, "classification")["label"], "positive")
        with self.assertRaises(m.AcceptanceFailure):
            m.validate_typed({**good, "confidence": 2.0}, "classification")
        with self.assertRaises(m.AcceptanceFailure):
            m.validate_typed({**good, "all_labels": []}, "classification")

    def test_fixture_is_confined_and_nosymlink(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            (base / "models").mkdir()
            fixture = base / "models" / "item.gguf"
            fixture.write_bytes(b"fake")
            self.assertEqual(m.fixture_path(base, "models/item.gguf"), fixture)
            for attack in ("../secret", "models/../../secret", "/tmp/other", "other.gguf"):
                with self.assertRaises(m.AcceptanceFailure):
                    m.fixture_path(base, attack)
            try:
                (base / "models" / "link.gguf").symlink_to(fixture)
            except (OSError, NotImplementedError):
                pass
            else:
                with self.assertRaises(m.AcceptanceFailure):
                    m.fixture_path(base, "models/link.gguf")

    def test_environment_disables_insecure_configuration(self):
        import os
        old = os.environ.get("GG_CORE_ALLOW_INSECURE_NO_AUTH")
        os.environ["GG_CORE_ALLOW_INSECURE_NO_AUTH"] = "1"
        try:
            env = m.clean_environment("/tmp/sock", "secret", 4096)
        finally:
            if old is None:
                del os.environ["GG_CORE_ALLOW_INSECURE_NO_AUTH"]
            else:
                os.environ["GG_CORE_ALLOW_INSECURE_NO_AUTH"] = old
        self.assertNotIn("GG_CORE_ALLOW_INSECURE_NO_AUTH", env)
        self.assertNotIn("GG_CORE_SANDBOX", env)
        self.assertEqual(env["CORE_AUTH_TOKEN"], "secret")
        self.assertEqual(env["GG_CORE_N_CTX"], "4096")


class OfflineOrchestrationTests(unittest.TestCase):
    def test_full_harness_against_fake_cli_contract(self):
        """Tests harness orchestration only, never claims model or daemon qualification."""
        if __import__('os').name == 'nt':
            self.skipTest('fake interpreter shebang test is Unix-only')
        import hashlib
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            (base / 'models').mkdir()
            fixture = base / 'models' / 'model.gguf'
            fixture.write_bytes(b'fake model bytes')
            binary = base / 'fake-cli'
            binary.write_text('''#!/usr/bin/env python3
import os, sys, json, signal, pathlib, time
args = sys.argv[1:]
marker = pathlib.Path(os.environ.get("GG_CORE_SOCKET_PATH", ""))
if args[:2] == ["version", "--json"]:
    print(json.dumps({"name":"GG-CORE","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","version":"1.0.0","features":["gguf","onnx"],"protocol_version":"V1","target":"linux-x86_64"}))
elif args[:2] == ["config", "validate"]:
    sys.exit(0)
elif args[0] == "serve":
    model = args[args.index("--model-id") + 1]
    marker.write_text(model + "\\n" + os.environ["CORE_AUTH_TOKEN"])
    def halt(signum, frame):
        marker.unlink(missing_ok=True)
        sys.exit(0)
    signal.signal(signal.SIGINT, halt)
    while True: time.sleep(0.1)
else:
    lines = marker.read_text().splitlines() if marker.exists() else []
    model = lines[0] if lines else "none"
    token = lines[1] if len(lines)>1 else ""
    if args[0] == "live":
        sys.exit(0 if lines else 1)
    if args[0] == "ready":
        sys.exit(0 if model != "none" else 1)
    if args[:2] == ["status", "--json"]:
        print(json.dumps({"health":"ready"}))
    elif args[:3] == ["models", "list", "--json"]:
        print(json.dumps({"models":[{"name":model,"capabilities":["text_generation"]}] if model != "none" else []}))
    elif args[:2] == ["models", "unload"]:
        marker.write_text("none\\n" + token)
    elif args[:2] == ["models", "load"]:
        marker.write_text(args[args.index("--id")+1] + "\\n" + token)
    elif args[0] == "infer":
        if token != os.environ.get("CORE_AUTH_TOKEN", "") or model == "none": sys.exit(1)
        print("flower")
    else: sys.exit(1)
''', encoding='utf-8')
            binary.chmod(0o700)
            args = m.parse_args([
                '--cli', str(binary), '--base', str(base), '--model', 'models/model.gguf',
                '--model-sha256', hashlib.sha256(fixture.read_bytes()).hexdigest(),
                '--cli-sha256', hashlib.sha256(binary.read_bytes()).hexdigest(),
                '--model-id', 'one', '--commit', 'a'*40, '--task', 'generation',
                '--startup-timeout', '5', '--inference-timeout', '5'
            ])
            evidence = m.execute(args)
            self.assertEqual(evidence['result'], 'PASS')
            self.assertIn('after_unload_reload', evidence['results'])
            self.assertEqual(len(evidence['checks']), 12)


if __name__ == "__main__":
    unittest.main()
