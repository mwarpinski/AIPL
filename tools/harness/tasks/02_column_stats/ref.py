"""The reference solution of 02_column_stats: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    lines = inp.split("\n")
    name = lines[0]
    rows = [l for l in lines[1:] if l != ""]
    header = rows[0].split(",")
    if name not in header:
        return f"no column {name}\n"
    k = header.index(name)
    vals = [int(r.split(",")[k]) for r in rows[1:]]
    if not vals:
        return "count 0\n"
    return f"count {len(vals)}\nsum {sum(vals)}\nmean {sum(vals) / len(vals):.2f}\nmin {min(vals)}\nmax {max(vals)}\n"

def cases(rng):
    out = ["price\nitem,price,qty\napple,3,10\npear,4,2\nfig,-1,5\n",
           "weight\nitem,price\na,1\n", "b\na,b,c\n", "x\nx\n7\n"]
    for _ in range(4):
        cols = ["c%d" % i for i in range(rng.randint(1, 6))]
        rows = [",".join(str(rng.randint(-1000, 1000)) for _ in cols) for _ in range(rng.randint(1, 300))]
        out.append(rng.choice(cols) + "\n" + ",".join(cols) + "\n" + "\n".join(rows) + "\n")
    return out
