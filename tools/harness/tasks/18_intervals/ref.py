"""The reference solution of 18_intervals: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    iv = sorted(tuple(map(int, l.split())) for l in inp.split("\n")[:-1])
    out = []
    for a, b in iv:
        if out and a <= out[-1][1]:
            out[-1][1] = max(out[-1][1], b)
        else:
            out.append([a, b])
    return ''.join(f"{a} {b}\n" for a, b in out)

def cases(rng):
    out = ["5 8\n1 3\n2 4\n10 10\n8 9\n", "", "1 3\n4 5\n", "1 3\n3 5\n"]
    for n, span in [(30, 100), (5000, 1000000)]:
        lines = []
        for _ in range(n):
            a = rng.randint(-span, span); b = a + rng.randint(0, span // 20)
            lines.append(f"{a} {b}")
        out.append("\n".join(lines) + "\n")
    return out
