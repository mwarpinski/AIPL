"""The reference solution of 21_edit_distance: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def dist(a, b):
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]

def solve(inp):
    out = []
    for l in inp.split("\n")[:-1]:
        a, b = l.split(" ")
        out.append(str(dist(a, b)))
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["kitten sitting\nflaw lawn\nabc abc\n", " abc\nabc \n \n"]
    for n, maxlen in [(30, 15), (20, 300)]:
        lines = []
        for _ in range(n):
            a = "".join(rng.choice("abcd") for _ in range(rng.randint(0, maxlen)))
            b = "".join(rng.choice("abcd") for _ in range(rng.randint(0, maxlen)))
            lines.append(f"{a} {b}")
        out.append("\n".join(lines) + "\n")
    return out
