"""HOL Phase 2 measurements (offline Windows VM).

usage: python -I -B run.py <gg-core-cli.exe> <verified models dir> <work dir> [concurrency|multimodel|all|policy|policy-all]

Stages a confined base directory whose models/ tree hard-links the verified
weights (no copy), writes ONNX manifests, then runs the requested phases and
writes <work>/results.json. Stdlib only; no network.
"""
import hashlib
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import concurrency  # noqa: E402
import multimodel  # noqa: E402

ONNX_META = {'minilm': (['embedding'], None), 'distilbert-sst2': (['text_classification'], ['NEGATIVE', 'POSITIVE'])}


def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for blk in iter(lambda: f.read(1 << 20), b''):
            h.update(blk)
    return h.hexdigest()


def stage(src, base):
    """Hard-link every verified file into base/models and check its hash."""
    prov = json.loads((src / 'PROVENANCE.json').read_text(encoding='utf-8'))
    checks = {}
    for entry in prov.values():
        for rel, info in entry['files'].items():
            dst = base / 'models' / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            if not dst.exists():
                os.link(src / rel, dst)
            checks[rel] = sha(dst) == info['sha256']
    for mid, (caps, labels) in ONNX_META.items():
        w = base / 'models' / mid / 'model.onnx'
        manifest = {'model_id': mid, 'name': mid, 'version': '1.0.0', 'capabilities': caps, 'sha256': sha(w),
                    'size_bytes': w.stat().st_size, 'architecture': 'onnx', 'license': 'Apache-2.0', 'labels': labels}
        (w.parent / 'model.onnx.manifest.json').write_text(json.dumps(manifest), encoding='utf-8')
    return checks


def main():
    exe, src, work = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    phase = sys.argv[4] if len(sys.argv) > 4 else 'all'
    base, logs = work / 'base', work / 'logs'
    logs.mkdir(parents=True, exist_ok=True)
    result = {'exe_sha256': sha(exe), 'staged': stage(src, base)}
    if not all(result['staged'].values()):
        raise SystemExit(f'model hash mismatch: {result["staged"]}')
    if phase in ('concurrency', 'all'):
        result['concurrency'] = concurrency.sweep(exe, str(base), str(logs))
    if phase in ('policy', 'policy-all'):
        result['concurrency'] = concurrency.sweep(exe, str(base), str(logs), concurrency.POLICY)
    if phase == 'policy-all':
        phase = 'multimodel'
    if phase in ('multimodel', 'all'):
        result['multimodel'] = [multimodel.run(exe, str(base), str(logs / f'daemon-mm-c{n}.log'), n)
                                for n in (2, 4)]
    (work / 'results.json').write_text(json.dumps(result, indent=1), encoding='utf-8')
    print('DONE', flush=True)


if __name__ == '__main__':
    main()
