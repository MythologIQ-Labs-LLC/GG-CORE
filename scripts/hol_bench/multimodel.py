"""Multi-model residency and mixed concurrent load under the default memory
total: admission refusals, peak private bytes vs budget, allocation failures
and idle-memory drift (leaks)."""
import re
import subprocess
import threading
import time

import ipc

GGUF = {
    'qwen': 'models/qwen/qwen2.5-0.5b-instruct-q4_k_m.gguf',
    'smollm2': 'models/smollm2/SmolLM2-1.7B-Instruct-Q4_K_M.gguf',
    'phi35': 'models/phi35/Phi-3.5-mini-instruct-Q4_K_M.gguf',
}
ONNX = {'minilm': 'models/minilm/model.onnx', 'distilbert-sst2': 'models/distilbert-sst2/model.onnx'}
ALLOC_FAIL = re.compile(r'failed to allocate|out of memory|alloc(ation)? fail|ggml_aligned_malloc|'
                        r'unable to allocate', re.I)


def prompt(model, q):
    if model == 'phi35':
        return f'<|user|>\n{q}<|end|>\n<|assistant|>\n'
    return f'<|im_start|>user\n{q}<|im_end|>\n<|im_start|>assistant\n'


def call(pipe, token, rid, job):
    c = ipc.Conn(pipe, token)
    try:
        kind, model = job
        if kind == 'gen':
            r = ipc.generate(c, rid, model, prompt(model, 'Name three rivers in Europe.'), 64, True)
        elif kind == 'embed':
            r = ipc.typed(c, rid, model, 'embedding', 'A man is eating food.')
        else:
            r = ipc.typed(c, rid, model, 'text_classification', 'I love this movie.')
    finally:
        c.close()
    return {'kind': kind, 'model': model, 'ms': round(r['total_ms'], 1), 'error': r['error']}


MIX = [('gen', 'qwen'), ('gen', 'smollm2'), ('gen', 'phi35'), ('embed', 'minilm'),
       ('classify', 'distilbert-sst2'), ('gen', 'qwen'), ('gen', 'smollm2'), ('gen', 'phi35')]


def mixed_round(pipe, token, rnd):
    rows, lock = [], threading.Lock()

    def go(i, job):
        r = call(pipe, token, 5000 + rnd * 100 + i, job)
        with lock:
            rows.append(r)
    ts = [threading.Thread(target=go, args=(i, j)) for i, j in enumerate(MIX)]
    for t in ts:
        t.start()
    for t in ts:
        t.join()
    return rows


def settle(smp, secs=6):
    time.sleep(secs)
    r = smp.read()
    return r['private'] if r else None


def budget(exe, base, env):
    p = subprocess.run([exe, 'config', 'show'], cwd=base, env=env, capture_output=True, text=True)
    m = re.search(r'GG_CORE_MAX_TOTAL_MEMORY=(\d+)', p.stdout)
    return int(m.group(1)) if m else None


def loads(pipe, token, smp):
    out = []
    for mid, path in list(GGUF.items())[1:] + list(ONNX.items()):
        c = ipc.Conn(pipe, token)
        try:
            r = ipc.load_model(c, path, mid)
        finally:
            c.close()
        r['private_after'] = settle(smp, 2)
        out.append(r)
    return out


def reload_cycles(pipe, token, smp, cycles=2):
    out = []
    for i in range(cycles):
        c = ipc.Conn(pipe, token)
        try:
            c.send({'type': 'model_unload_request', 'model_id': 'phi35'}); c.recv()
            before = settle(smp, 4)
            r = ipc.load_model(c, GGUF['phi35'], 'phi35')
        finally:
            c.close()
        out.append({'cycle': i, 'private_unloaded': before, 'reload_ok': r['success'],
                    'reload_error': r['error'], 'infer': call(pipe, token, 7000 + i, ('gen', 'phi35'))})
    return out


def run(exe, base, log, max_concurrent):
    env = {'GG_CORE_N_CTX': '4096', 'GG_CORE_MAX_CONTEXT': '4096', 'GG_CORE_MAX_CONTEXT_TOKENS': '4096',
           'GG_CORE_MAX_CONCURRENT': str(max_concurrent), 'PATH': r'C:\host\vcdlls;' + ipc.SYS_PATH}
    total = budget(exe, base, {**env, 'SystemRoot': r'C:\Windows'})
    proc, pipe, token, ready = ipc.daemon(exe, base, dict(env), log, preload=(GGUF['qwen'], 'qwen'))
    out = {'max_concurrent': max_concurrent, 'ready': ready, 'budget_total_bytes': total}
    if not ready:
        return out
    smp = ipc.Sampler(proc.pid); smp.start()
    try:
        out['idle_one_model'] = settle(smp)
        out['loads'] = loads(pipe, token, smp)
        out['idle_after_loads'] = settle(smp)
        t0 = ipc.now_ms()
        out['rounds'] = [mixed_round(pipe, token, r) for r in range(3)]
        out['mixed_window'] = smp.window(t0, ipc.now_ms())
        out['idle_after_workload'] = settle(smp)
        out['reload_cycles'] = reload_cycles(pipe, token, smp)
        out['idle_end'] = settle(smp)
        out['peak_private'] = max(r['private'] for r in smp.rows)
        out['daemon_alive'] = proc.poll() is None
    finally:
        smp.stop.set(); proc.terminate(); proc.wait(60)
    text = open(log, encoding='utf-8', errors='replace').read()
    out['alloc_failure_lines'] = len(ALLOC_FAIL.findall(text))
    return out
