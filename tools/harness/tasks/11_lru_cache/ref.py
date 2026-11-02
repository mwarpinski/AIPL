"""The reference solution of 11_lru_cache: it defines the expected outputs
(tests/), and is never shown to a model."""
import random
from collections import OrderedDict

def solve(inp):
    lines = inp.split("\n")[:-1]
    cap = int(lines[0])
    c = OrderedDict(); out = []; hits = misses = 0
    for l in lines[1:]:
        p = l.split()
        k = int(p[1])
        if p[0] == "get":
            if k in c:
                c.move_to_end(k); out.append(str(c[k])); hits += 1
            else:
                out.append("-1"); misses += 1
        else:
            if k in c:
                c.move_to_end(k)
            elif len(c) >= cap:
                c.popitem(last=False)
            c[k] = int(p[2])
    out.append(f"hits {hits} misses {misses}")
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["2\nput 1 10\nput 2 20\nget 1\nput 3 30\nget 2\nget 3\n", "1\n", "1\nget 5\nput 5 1\nput 6 2\nget 5\nget 6\n"]
    for cap, n, keys in [(3, 50, 6), (100, 5000, 300), (1000, 20000, 1500)]:
        lines = [str(cap)]
        for _ in range(n):
            if rng.random() < 0.5:
                lines.append(f"get {rng.randrange(keys)}")
            else:
                lines.append(f"put {rng.randrange(keys)} {rng.randint(-1000, 1000)}")
        out.append("\n".join(lines) + "\n")
    return out
