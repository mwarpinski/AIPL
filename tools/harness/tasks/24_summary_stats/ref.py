"""The reference solution of 24_summary_stats: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, math
from collections import Counter

def solve(inp):
    v = [int(l) for l in inp.split("\n")[:-1]]
    n = len(v); s = sorted(v)
    mean = sum(v) / n
    med = s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2
    c = Counter(v); top = max(c.values())
    mode = min(k for k, x in c.items() if x == top)
    sd = math.sqrt(sum((x - mean) ** 2 for x in v) / n)
    return f"mean {mean:.2f}\nmedian {med:.1f}\nmode {mode}\nstddev {sd:.3f}\nrange {s[-1] - s[0]}\n"

def cases(rng):
    out = ["2\n4\n4\n5\n", "7\n", "-3\n3\n"]
    for n in [11, 500, 5000]:
        out.append("".join(f"{rng.randint(-1000, 1000)}\n" for _ in range(n)))
    return out
