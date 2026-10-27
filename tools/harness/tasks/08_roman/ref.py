"""The reference solution of 08_roman: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

VALS = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]

def to_roman(n):
    s = ""
    for v, r in VALS:
        while n >= v:
            s += r; n -= v
    return s

def from_roman(s):
    n = 0; i = 0
    for v, r in VALS:
        while s.startswith(r, i):
            n += v; i += len(r)
    return n

def solve(inp):
    out = []
    for l in inp.split("\n")[:-1]:
        out.append(to_roman(int(l)) if l.isdigit() else str(from_roman(l)))
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["1994\nMMXXVI\n4\n", "1\n3999\nMMMCMXCIX\nI\n"]
    for _ in range(3):
        lines = []
        for _ in range(rng.randint(5, 50)):
            n = rng.randint(1, 3999)
            lines.append(str(n) if rng.random() < 0.5 else to_roman(n))
        out.append("\n".join(lines) + "\n")
    return out
