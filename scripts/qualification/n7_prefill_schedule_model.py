"""N7 deterministic prefill schedule model (plan Revisions 3 and 4).

Usage: python -I n7_prefill_schedule_model.py CAL_FILE [--integer]

Default: the float model (fit check, transitions every 16 positions, worst
chunk and cost against fixed 512). --integer: the exact integer formula used
by core-runtime/src/engine/gguf/prefill_schedule.rs, printing its TABLE and
transitions at every position (the source of the unit-test expectations).
"""
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


def integer_mode():
    t0x2 = {k: sum(v[0]) for k, v in rows.items() if k[0] in SIZES}
    t16x2 = {k: 2 * v[13926][0] for k, v in rows.items() if k[0] in SIZES}
    ref, span, budget_x2 = 485_452_288, 13926, 2000
    for t in [1, 2, 4, 8]:
        print("    [" + ", ".join(f"[{t0x2[(z, t)]}, {t16x2[(z, t)]}]" for z in SIZES) + f"],  // {t} thread(s)")
    def ichunk(p, t):
        for z in SIZES:
            a, b = t0x2[(z, t)], t16x2[(z, t)]
            if (a * span + (b - a) * p) * ref <= budget_x2 * span * ref:
                return z
        return 64
    for t in [1, 2, 4, 8]:
        trans, prev = [], None
        for p in range(0, 16384):
            c = ichunk(p, t)
            if c != prev:
                trans.append((p, c)); prev = c
        print(f"integer bucket {t}: transitions {trans}")
    # Revision 4: sub-64 remainder pieces, p_max = 24*B*D/N - 6 (18 if !fits).
    d = 2 * span * ref
    def pmax(p, t, b):
        a, c = t0x2[(64, t)], t16x2[(64, t)]
        n = (a * span + (c - a) * p) * b
        return 10**9 if n == 0 else 18 if n >= 1000 * d else 24 * 1000 * d // n - 6
    def seq(n, t, b=ref):
        pos, out = 0, []
        while pos < n:
            rem = n - pos; s = min(ichunk(p=pos, t=t) if b == ref else bulk(pos, t, b), rem)
            if rem < 64:
                piece = max(1, min(pmax(pos, t, b), s)); q = -(-rem // piece); s = -(-rem // q)
            out.append(s); pos += s
        return out
    def bulk(p, t, b):
        for z in SIZES:
            a, c = t0x2[(z, t)], t16x2[(z, t)]
            if (a * span + (c - a) * p) * b <= budget_x2 * span * ref:
                return z
        return 64
    for t in [1, 2, 4, 8]:
        for n in [2516, 568, 100, 63]:
            print(f"rev4 bucket {t} n={n}: {seq(n, t)}")
    for name, b in [("phi35", 2_392_493_952), ("smollm2", 1_053_827_072)]:
        for t in [1, 4]:
            print(f"rev4 {name} bucket {t} n=2877 tail: {seq(2877, t, b)[-6:]}")

if "--integer" in sys.argv:
    integer_mode()
