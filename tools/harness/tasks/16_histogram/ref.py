"""The reference solution of 16_histogram: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    lines = inp.split("\n")[:-1]
    lo, hi, b = map(int, lines[0].split())
    w = (hi - lo) // b
    counts = [0] * b; below = above = 0
    for l in lines[1:]:
        v = int(l)
        if v < lo: below += 1
        elif v >= hi: above += 1
        else: counts[(v - lo) // w] += 1
    out = []
    for i, c in enumerate(counts):
        s = f"[{lo + i * w},{lo + (i + 1) * w}) {c}"
        out.append(s + (" " + "#" * c if c else ""))
    out += [f"below {below}", f"above {above}"]
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["0 10 2\n3\n7\n4\n10\n-1\n", "-10 10 4\n", "0 1 1\n0\n0\n1\n"]
    for lo, w, b, n in [(-50, 10, 10, 60), (100, 25, 8, 150)]:
        hi = lo + w * b
        vals = [rng.randint(lo - 20, hi + 20) for _ in range(n)]
        out.append(f"{lo} {hi} {b}\n" + "".join(f"{v}\n" for v in vals))
    return out
