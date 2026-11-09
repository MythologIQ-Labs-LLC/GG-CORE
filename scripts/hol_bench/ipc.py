"""Stdlib-only GG-CORE IPC client, daemon launcher and memory sampler for the
HOL concurrency / multi-model measurements (Windows named pipe).

Never writes the auth token to disk; raw model output stays in memory and is
only hashed or compared.
"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import secrets
import struct
import subprocess
import threading
import time

K32 = ctypes.WinDLL('kernel32', use_last_error=True)
SYS_PATH = r'C:\Windows\System32;C:\Windows;C:\Windows\System32\WindowsPowerShell\v1.0'


def now_ms():
    return time.perf_counter() * 1000.0


class Conn:
    """One authenticated connection; frames are 4-byte LE length + JSON."""

    def __init__(self, pipe, token, attempts=400):
        for _ in range(attempts):
            try:
                self.f = open(pipe, 'r+b', buffering=0)
                break
            except OSError:  # pipe busy / not yet listening: retry
                time.sleep(0.05)
        else:
            raise OSError('pipe never became available')
        self.send({'type': 'handshake', 'token': token, 'protocol_version': 'V1'})
        ack = self.recv()
        if ack.get('type') != 'handshake_ack':
            raise RuntimeError(f'handshake refused: {ack.get("type")}')

    def send(self, msg):
        data = json.dumps(msg).encode()
        self.f.write(struct.pack('<I', len(data)) + data)

    def _exact(self, n):
        buf = b''
        while len(buf) < n:
            part = self.f.read(n - len(buf))
            if not part:
                raise EOFError('connection closed')
            buf += part
        return buf

    def recv(self):
        (n,) = struct.unpack('<I', self._exact(4))
        return json.loads(self._exact(n))

    def close(self):
        try:
            self.f.close()
        except OSError:
            pass


def generate(conn, rid, model, prompt, max_tokens, stream, timeout_ms=600_000):
    """Run one generation; returns timing, text and error (streaming or not)."""
    params = {'max_tokens': max_tokens, 'temperature': 0.0, 'top_p': 1.0, 'top_k': 1,
              'stream': stream, 'timeout_ms': timeout_ms}
    t0 = now_ms()
    conn.send({'type': 'inference_request', 'request_id': rid, 'model_id': model,
               'prompt': prompt, 'parameters': params})
    if not stream:
        r = conn.recv()
        return {'total_ms': now_ms() - t0, 'ttft_ms': None, 'text': r.get('output', ''),
                'tokens': r.get('tokens_generated'), 'error': r.get('error') or _frame_error(r)}
    ttft, parts = None, []
    while True:
        c = conn.recv()
        if c.get('type') != 'stream_chunk':
            return {'total_ms': now_ms() - t0, 'ttft_ms': ttft, 'text': ''.join(parts),
                    'chunks': len(parts), 'error': _frame_error(c)}
        if c.get('text'):
            ttft = ttft if ttft is not None else now_ms() - t0
            parts.append(c['text'])
        if c.get('is_final'):
            return {'total_ms': now_ms() - t0, 'ttft_ms': ttft, 'text': ''.join(parts),
                    'chunks': len(parts), 'error': c.get('error')}


def typed(conn, rid, model, task, text, timeout_ms=600_000):
    t0 = now_ms()
    conn.send({'type': 'typed_inference_request', 'request_id': rid, 'model_id': model,
               'task': task, 'input': {'text': text}, 'timeout_ms': timeout_ms})
    r = conn.recv()
    return {'total_ms': now_ms() - t0, 'error': r.get('error') or _frame_error(r),
            'ok': r.get('result') is not None}


def load_model(conn, path, model_id):
    t0 = now_ms()
    conn.send({'type': 'model_load_request', 'path': path, 'model_id': model_id})
    r = conn.recv()
    return {'model': model_id, 'ms': round(now_ms() - t0, 1), 'success': r.get('success'),
            'error': r.get('error') or _frame_error(r)}


def _frame_error(frame):
    if frame.get('type') == 'error':
        return f'error {frame.get("code")}: {frame.get("message")}'
    return None


def daemon(exe, base, env_extra, log_path, preload=None):
    """Start `gg-core-cli serve`; returns (proc, pipe, token, ready)."""
    token, pipe = secrets.token_hex(24), r'\\.\pipe\gg-v1bench-' + secrets.token_hex(6)
    env = {k: v for k, v in os.environ.items() if not k.startswith('GG_CORE_')}
    env.update({'PATH': env_extra.pop('PATH', SYS_PATH), 'CORE_AUTH_TOKEN': token,
                'GG_CORE_SOCKET_PATH': pipe, **env_extra})
    args = [exe, 'serve'] + (['--model', preload[0], '--model-id', preload[1]] if preload else [])
    log = open(log_path, 'w', encoding='utf-8')
    proc = subprocess.Popen(args, cwd=base, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, encoding='utf-8', errors='replace')
    ready = threading.Event()

    def pump():
        for line in proc.stdout:
            log.write(line); log.flush()
            if 'IPC server listening' in line:
                ready.set()
    threading.Thread(target=pump, daemon=True).start()
    ready.wait(900)
    return proc, pipe, token, ready.is_set()


class PMC(ctypes.Structure):
    _fields_ = [('cb', wt.DWORD), ('PageFaultCount', wt.DWORD)] + [
        (n, ctypes.c_size_t) for n in ('PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                                       'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage',
                                       'QuotaNonPagedPoolUsage', 'PagefileUsage', 'PeakPagefileUsage',
                                       'PrivateUsage')]


class Sampler(threading.Thread):
    """Samples the daemon's private bytes and CPU seconds every 100 ms."""

    def __init__(self, pid):
        super().__init__(daemon=True)
        self.h = K32.OpenProcess(0x1000, False, pid)
        self.rows, self.stop = [], threading.Event()

    def read(self):
        c = PMC(); c.cb = ctypes.sizeof(PMC)
        if not K32.K32GetProcessMemoryInfo(self.h, ctypes.byref(c), c.cb):
            return None
        ft = [wt.FILETIME() for _ in range(4)]
        K32.GetProcessTimes(self.h, *[ctypes.byref(x) for x in ft])
        cpu = sum((x.dwHighDateTime << 32 | x.dwLowDateTime) for x in ft[2:]) / 1e7
        return {'t': now_ms(), 'private': c.PrivateUsage, 'cpu_s': cpu}

    def run(self):
        while not self.stop.is_set():
            r = self.read()
            if r:
                self.rows.append(r)
            time.sleep(0.1)

    def window(self, t0, t1):
        rows = [r for r in self.rows if t0 <= r['t'] <= t1]
        if len(rows) < 2:
            return {}
        cpu = rows[-1]['cpu_s'] - rows[0]['cpu_s']
        return {'peak_private': max(r['private'] for r in rows),
                'avg_cores_busy': round(cpu / ((rows[-1]['t'] - rows[0]['t']) / 1000), 2)}
