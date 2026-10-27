"""The reference solution of 20_days_between: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, datetime

def solve(inp):
    out = []
    for l in inp.split("\n")[:-1]:
        a, b = (datetime.date.fromisoformat(x) for x in l.split())
        out.append(str((b - a).days))
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["2024-02-28 2024-03-01\n2023-02-28 2023-03-01\n2000-01-01 1999-12-31\n",
           "1600-01-01 2400-12-31\n1900-02-28 1900-03-01\n2100-02-28 2100-03-01\n"]
    lo, hi = datetime.date(1600, 1, 1).toordinal(), datetime.date(2400, 12, 31).toordinal()
    for _ in range(3):
        lines = []
        for _ in range(rng.randint(5, 100)):
            a = datetime.date.fromordinal(rng.randint(lo, hi)); b = datetime.date.fromordinal(rng.randint(lo, hi))
            lines.append(f"{a.isoformat()} {b.isoformat()}")
        out.append("\n".join(lines) + "\n")
    return out
