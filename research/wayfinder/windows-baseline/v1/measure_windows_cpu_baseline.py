#!/usr/bin/env python3
"""Measure packaged GG-CORE CLI round-trip timing for immutable synthetic fixtures.

Offline, stdlib-only. Does NOT launch a daemon, download a model, verify OS
isolation, benchmark GPU, or claim a quality score. --run executes a previously
verified CLI against an already-running local authenticated daemon.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

COMMIT = 'fd9ee79574dfca8ba860b2e579bbce5555f0c6b0'
EXE_SHA = '69f78946ba5133cc39a69d20a7092a0d717bccd6e960cc758c945d271b13f402'
MODEL_SHA = '74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db'
FIXTURE_SHA = '58e17222c7211d7af32f9bf01a85ed95e52d17a733754d4547be18825dad970f'
EXPECTED_IDS = ['F01', 'S01', 'D01', 'C01', 'A01', 'X01']


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def check_fixtures(path):
    if digest(path) != FIXTURE_SHA:
        raise RuntimeError('Fixture JSONL digest mismatch; do not normalize or edit the inputs')
    rows = [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()]
    if [r['id'] for r in rows] != EXPECTED_IDS:
        raise RuntimeError('Unexpected fixture IDs/order')
    for row in rows:
        prompt = row['prompt'].encode('utf-8')
        if (not row['synthetic_only'] or len(prompt) != row['prompt_bytes']
                or hashlib.sha256(prompt).hexdigest() != row['prompt_sha256']):
            raise RuntimeError('Fixture prompt hash, byte count or synthetic marker mismatch: ' + row['id'])
    return rows


def check_release_archives(directory):
    manifest = directory / 'SHA256SUMS'
    if not manifest.is_file():
        raise RuntimeError('Missing SHA256SUMS in release dir')
    expected = {
        'gg-core-v0.10.0-rc.1-linux-x86_64.tar.gz': '0673c8c4bba233aa2d2bf9d3f5195a02fb2bcfeba6c5e5a6635bfb079d9f9c5b',
        'gg-core-v0.10.0-rc.1-windows-x86_64.tar.gz': '71abb8c8b23fe5a1d5719f3f97318a28df445466db4c680134da407de67af07c',
        'gg-core-v0.10.0-rc.1-macos-arm64.tar.gz': 'c73179b4ed0638579e91cffb7a617a4612c47c786e94a1174177c8dbdc1dad1e',
    }
    actual = {}
    for line in manifest.read_text(encoding='utf-8').splitlines():
        bits = line.split(maxsplit=1)
        if len(bits) != 2:
            raise RuntimeError('Malformed SHA256SUMS line')
        file_name = bits[1].lstrip('* ')
        if file_name in actual or file_name not in expected or bits[0] != expected[file_name]:
            raise RuntimeError('Unexpected release manifest entry')
        actual[file_name] = bits[0]
    if actual != expected:
        raise RuntimeError('Manifest missing expected release platforms')
    for file_name, sha in expected.items():
        if digest(directory / file_name) != sha:
            raise RuntimeError('Release archive hash mismatch: ' + file_name)
    print('PASS: all 3 platform archives match the pinned published SHA256SUMS')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--fixtures', type=Path, required=True)
    p.add_argument('--release-dir', type=Path, help='Optional directory with all four published release assets')
    p.add_argument('--run', action='store_true', help='Explicitly enable execution after external Dev isolation approval')
    p.add_argument('--dev-isolation-approved', action='store_true', help='Operator attests separately verified restricted Dev token/ACL isolation; this flag cannot prove isolation')
    p.add_argument('--cli', type=Path, help='Exact extracted published gg-core-cli.exe')
    p.add_argument('--model-file', type=Path, help='Already downloaded, locally authorized pinned Qwen 0.5B model file')
    p.add_argument('--model-id', default='qwen')
    p.add_argument('--out', type=Path, help='New local-only JSONL result file; must not exist')
    p.add_argument('--repetitions', type=int, default=3)
    p.add_argument('--max-tokens', type=int, default=256)
    p.add_argument('--timeout-seconds', type=int, default=240)
    a = p.parse_args()
    rows = check_fixtures(a.fixtures)
    print(f'PASS: all {len(rows)} immutable synthetic fixture records and hashes')
    if a.release_dir:
        check_release_archives(a.release_dir)
    if not a.run:
        print('VERIFY-ONLY; no daemon or inference process started')
        return 0
    if os.name != 'nt':
        raise RuntimeError('Windows benchmark execution only; do not treat another OS as Windows evidence')
    if not a.dev_isolation_approved:
        raise RuntimeError('Missing externally verified Dev isolation approval; flag alone is NOT security evidence')
    if not a.cli or not a.model_file or not a.out:
        raise RuntimeError('--cli, --model-file, --out are required for --run')
    if not (1 <= a.repetitions <= 100 and 1 <= a.max_tokens <= 4096 and a.timeout_seconds > 0):
        raise RuntimeError('Invalid repetitions/max-tokens/timeout')
    if a.out.exists():
        raise RuntimeError('Refuse to overwrite existing evidence file')
    if digest(a.cli) != EXE_SHA:
        raise RuntimeError('Windows CLI does not match published executable SHA256')
    if digest(a.model_file) != MODEL_SHA:
        raise RuntimeError('Qwen 0.5B fixture SHA256 mismatch')
    if len(os.environ.get('CORE_AUTH_TOKEN', '').encode('utf-8')) < 16:
        raise RuntimeError('CORE_AUTH_TOKEN missing/too short; do not print the token')
    if not os.environ.get('GG_CORE_SOCKET_PATH'):
        raise RuntimeError('GG_CORE_SOCKET_PATH missing; daemon must already be running securely')
    version = subprocess.run([str(a.cli), 'version', '--json'], capture_output=True,
                             text=True, timeout=30, check=True)
    meta = json.loads(version.stdout)
    if (meta.get('commit') != COMMIT or meta.get('version') != '0.10.0-rc.1'
            or 'gguf' not in meta.get('features', [])):
        raise RuntimeError('CLI version/commit/GGUF feature does not match frozen RC1')
    with a.out.open('x', encoding='utf-8', newline='\n') as dest:
        for fixture in rows:
            for rep in range(a.repetitions):
                command = [str(a.cli), 'infer', '--model', a.model_id,
                           '--prompt', fixture['prompt'], '--max-tokens', str(a.max_tokens)]
                started = time.perf_counter_ns()
                try:
                    outcome = subprocess.run(command, capture_output=True, text=True,
                                             timeout=a.timeout_seconds, check=False)
                    elapsed_ms = (time.perf_counter_ns() - started) / 1e6
                    result = {'exit_code': outcome.returncode, 'timed_out': False,
                              'response_sha256': hashlib.sha256(outcome.stdout.encode('utf-8')).hexdigest(),
                              'response_bytes': len(outcome.stdout.encode('utf-8')),
                              'stderr_sha256': hashlib.sha256(outcome.stderr.encode('utf-8')).hexdigest()}
                except subprocess.TimeoutExpired:
                    elapsed_ms = (time.perf_counter_ns() - started) / 1e6
                    result = {'exit_code': None, 'timed_out': True, 'response_sha256': None,
                              'response_bytes': None, 'stderr_sha256': None}
                record = {'fixture_id': fixture['id'], 'prompt_sha256': fixture['prompt_sha256'],
                          'repetition': rep + 1, 'producer_commit': COMMIT,
                          'exe_sha256': EXE_SHA, 'model_sha256': MODEL_SHA,
                          'total_cli_roundtrip_ms': round(elapsed_ms, 3),
                          'ttft_ms': None, 'itl_ms': None, 'rss_peak_bytes': None,
                          'vram_peak_bytes': None, 'quality_verdict': 'not_scored',
                          'security_boundary': 'externally_attested_not_verified_by_script', **result}
                dest.write(json.dumps(record, sort_keys=True) + '\n')
                dest.flush()
                print(f"{fixture['id']} rep={rep + 1} exit={record['exit_code']} elapsed_ms={record['total_cli_roundtrip_ms']}")
    print('CPU CLI round-trip sample only; no TTFT, native-memory or quality result is claimed')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError) as exc:
        print('FAIL: ' + str(exc), file=sys.stderr)
        sys.exit(2)
