"""The reference solution of 07_collatz: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    n = int(inp)
    cache = {1: 1}
    best, blen = 1, 1
    for s in range(1, n + 1):
        x, path = s, []
        while x not in cache:
            path.append(x)
            x = x // 2 if x % 2 == 0 else 3 * x + 1
        l = cache[x]
        for v in reversed(path):
            l += 1
            cache[v] = l
        if cache[s] > blen:
            best, blen = s, cache[s]
    return f"{best} {blen}\n"

def cases(rng):
    return ["10\n", "1\n", "2\n", "1000000\n", f"{rng.randint(1000, 300000)}\n"]
