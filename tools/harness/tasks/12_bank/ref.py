"""The reference solution of 12_bank: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    acct = {}; out = []
    for i, l in enumerate(inp.split("\n")[:-1], 1):
        p = l.split()
        op = p[0]
        def err(r):
            out.append(f"error {i}: {r}")
        if op == "open":
            if p[1] in acct: err("already open")
            else: acct[p[1]] = 0
        elif op == "deposit":
            if p[1] not in acct: err("no such account")
            else: acct[p[1]] += int(p[2])
        elif op == "withdraw":
            if p[1] not in acct: err("no such account")
            elif acct[p[1]] < int(p[2]): err("insufficient funds")
            else: acct[p[1]] -= int(p[2])
        else:
            a, b, amt = p[1], p[2], int(p[3])
            if a not in acct or b not in acct: err("no such account")
            elif acct[a] < amt: err("insufficient funds")
            else:
                acct[a] -= amt; acct[b] += amt
    for name in sorted(acct):
        c = acct[name]
        out.append(f"{name} {c // 100}.{c % 100:02d}")
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["open alice\ndeposit alice 1050\nopen bob\ntransfer alice bob 300\nwithdraw bob 500\nopen alice\ndeposit carol 5\n",
           "open z\n", "withdraw a 1\n"]
    names = ["ann", "bo", "cy", "dee", "ed", "flo", "gus"]
    for n in [40, 400, 3000]:
        lines = []
        for _ in range(n):
            r = rng.random()
            a, b = rng.choice(names), rng.choice(names)
            if r < 0.15: lines.append(f"open {a}")
            elif r < 0.45: lines.append(f"deposit {a} {rng.randint(1, 100000)}")
            elif r < 0.7: lines.append(f"withdraw {a} {rng.randint(1, 50000)}")
            else: lines.append(f"transfer {a} {b} {rng.randint(1, 50000)}")
        out.append("\n".join(lines) + "\n")
    return out
