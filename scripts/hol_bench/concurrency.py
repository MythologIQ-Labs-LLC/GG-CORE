"""Concurrency sweep: throughput, TTFT/latency percentiles and byte-identity
of streams against a serial baseline, per GG_CORE_MAX_CONCURRENT and
GG_CORE_N_THREADS setting. One resident model (Qwen2.5-0.5B, CPU GGUF)."""
import hashlib
import statistics
import threading
import time

import ipc

QWEN = ('models/qwen/qwen2.5-0.5b-instruct-q4_k_m.gguf', 'qwen')
SYSTEM = 'You are Qwen, created by Alibaba Cloud. You are a helpful assistant.'
QUESTIONS = [
    'Explain how a refrigerator keeps food cold.',
    'Write a short story about a lighthouse keeper.',
    'List ten uses for a paperclip, one per line.',
    'Describe the water cycle step by step.',
    'What are the main differences between a cat and a dog as pets?',
    'Give a recipe for a simple tomato soup.',
    'Explain what a binary search does, with an example.',
    'Summarize the plot of a heist movie you invent.',
]
MAX_TOKENS = 128


def chatml(q):
    return (f'<|im_start|>system\n{SYSTEM}<|im_end|>\n<|im_start|>user\n{q}<|im_end|>\n'
            '<|im_start|>assistant\n')


PROMPTS = [chatml(q) for q in QUESTIONS]


def pct(xs, p):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None
    k = max(0, min(len(xs) - 1, round(p / 100 * (len(xs) - 1))))
    return round(xs[k], 1)


def one(pipe, token, rid, i, stream=True):
    c = ipc.Conn(pipe, token)
    try:
        r = ipc.generate(c, rid, 'qwen', PROMPTS[i], MAX_TOKENS, stream)
    finally:
        c.close()
    r['prompt'] = i
    r['sha'] = hashlib.sha256(r['text'].encode()).hexdigest()
    return r


def baseline(pipe, token):
    """Serial, deterministic reference: token counts (non-streaming) and stream text."""
    tokens = {i: one(pipe, token, 9000 + i, i, stream=False)['tokens'] for i in range(len(PROMPTS))}
    streams = {i: one(pipe, token, 9100 + i, i)['sha'] for i in range(len(PROMPTS))}
    return tokens, streams


def concurrent(pipe, token, clients, total):
    """`clients` threads pull `total` requests; returns per-request rows and wall ms."""
    rows, lock, nxt = [], threading.Lock(), iter(range(total))

    def worker():
        while True:
            with lock:
                i = next(nxt, None)
            if i is None:
                return
            t0 = ipc.now_ms()
            r = one(pipe, token, 1000 + i, i % len(PROMPTS))
            r['start'], r['end'] = t0, ipc.now_ms()
            with lock:
                rows.append(r)
    ts = [threading.Thread(target=worker) for _ in range(clients)]
    t0 = ipc.now_ms()
    for t in ts:
        t.start()
    for t in ts:
        t.join()
    return rows, t0, ipc.now_ms()


def summarize(rows, t0, t1, tokens, ref):
    ok = [r for r in rows if not r['error']]
    toks = [tokens[r['prompt']] for r in ok]
    decode = [tokens[r['prompt']] / ((r['total_ms'] - r['ttft_ms']) / 1000)
              for r in ok if r['ttft_ms'] is not None and r['total_ms'] > r['ttft_ms']]
    return {'requests': len(rows), 'errors': [r['error'] for r in rows if r['error']],
            'wall_s': round((t1 - t0) / 1000, 2),
            'aggregate_tok_s': round(sum(toks) / ((t1 - t0) / 1000), 1),
            'per_request_decode_tok_s_median': round(statistics.median(decode), 1) if decode else None,
            'per_request_e2e_tok_s_median': round(statistics.median(
                [tokens[r['prompt']] / (r['total_ms'] / 1000) for r in ok]), 1) if ok else None,
            'ttft_ms_p50': pct([r['ttft_ms'] for r in ok], 50), 'ttft_ms_p95': pct([r['ttft_ms'] for r in ok], 95),
            'total_ms_p50': pct([r['total_ms'] for r in ok], 50), 'total_ms_p95': pct([r['total_ms'] for r in ok], 95),
            'byte_identical_to_serial': sum(1 for r in ok if r['sha'] == ref[r['prompt']]),
            'compared': len(ok)}


def run_config(exe, base, log, max_concurrent, n_threads, ref):
    env = {'GG_CORE_MAX_CONCURRENT': str(max_concurrent), 'GG_CORE_N_CTX': '4096',
           'GG_CORE_MAX_CONTEXT': '4096', 'GG_CORE_MAX_CONTEXT_TOKENS': '4096', 'PATH': ipc_path()}
    if n_threads:
        env['GG_CORE_N_THREADS'] = str(n_threads)
    proc, pipe, token, ready = ipc.daemon(exe, base, env, log, preload=QWEN)
    out = {'max_concurrent': max_concurrent, 'n_threads': n_threads or 'auto', 'ready': ready}
    try:
        if not ready:
            return out
        smp = ipc.Sampler(proc.pid); smp.start()
        one(pipe, token, 1, 0)  # warm-up
        tokens, serial = baseline(pipe, token)
        ref = ref or (tokens, serial)
        out['serial_byte_identical'] = sum(1 for i in serial if serial[i] == ref[1][i])
        rows, t0, t1 = concurrent(pipe, token, max_concurrent, max(16, 3 * max_concurrent))
        out.update(summarize(rows, t0, t1, ref[0], ref[1]), **smp.window(t0, t1))
        smp.stop.set()
        return out, ref
    finally:
        proc.terminate(); proc.wait(60)


def ipc_path():
    return r'C:\host\vcdlls;' + ipc.SYS_PATH


def sweep(exe, base, logdir):
    """Default threads at 1/2/4/8 workers, then explicit 12/N threads."""
    configs = [(1, None), (2, None), (4, None), (8, None), (2, 6), (4, 3), (8, 1)]
    results, ref = [], None
    for n, threads in configs:
        r = run_config(exe, base, f'{logdir}\\daemon-c{n}-t{threads or "auto"}.log', n, threads, ref)
        if isinstance(r, tuple):
            r, ref = r
        results.append(r)
        print('CONC', r, flush=True)
        time.sleep(2)
    return {'baseline_tokens': ref[0] if ref else None, 'configs': results}
