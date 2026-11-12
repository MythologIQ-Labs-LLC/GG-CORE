import re, sys
rows = {}
for l in open(sys.argv[1]):
    m = re.match(r'CAL n_ctx=(\d+) threads=(\d+) model_bytes=\d+ pos=(\d+) chunk=(\d+) ms=(\d+)', l)
    if m:
        c, t, p, s, ms = map(int, m.groups())
        rows.setdefault((s, t), {}).setdefault(p, []).append(ms)
T0 = {k: sum(v[0]) / len(v[0]) for k, v in rows.items()}
T16 = {k: v[13926][0] for k, v in rows.items()}
T8 = {k: v[6963][0] for k, v in rows.items()}
SIZES = [512, 256, 128, 64]
def ms(s, t, p): return T0[(s, t)] + (T16[(s, t)] - T0[(s, t)]) * p / 13926
for t in [1, 2, 4, 8]:
    for s in SIZES:
        print(f"check t={t} s={s} pred@6963={ms(s,t,6963):.0f} meas={T8[(s,t)]}")
def chunk(p, t):
    for s in SIZES:
        if ms(s, t, p) <= 1000: return s
    return 64
def run(n, t):
    p, seq, worst, total = 0, [], 0, 0
    while p < n:
        s = min(chunk(p, t), n - p)
        d = ms(chunk(p, t), t, p) * s / chunk(p, t)  # partial last chunk scaled
        seq.append((p, s)); worst = max(worst, d); total += d; p += s
    return seq, worst, total
def fixed(n, t, s=512):
    p, total = 0, 0
    while p < n:
        k = min(s, n - p); total += ms(s, t, p) * k / s; p += k
    return total
for t in [1, 2, 4, 8]:
    trans = []; prev = None
    for p in range(0, 16384, 16):
        c = chunk(p, t)
        if c != prev: trans.append((p, c)); prev = c
    print(f"t={t} transitions={trans}")
    for n in [2516, 13926, 16300]:
        seq, worst, total = run(n, t)
        f = fixed(n, t)
        print(f"  n={n} chunks={len(seq)} worst_ms={worst:.0f} total_ms={total:.0f} fixed512_ms={f:.0f} cost={100*(total/f-1):+.0f}%")
