"""The reference solution of 05_run_length: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, re, itertools

def solve(inp):
    out = []
    for line in inp.split("\n")[:-1]:
        op, text = line[0], line[2:]
        if op == 'E':
            out.append(''.join(f"{len(list(g))}{c}" for c, g in itertools.groupby(text)))
        else:
            out.append(''.join(c * int(n) for n, c in re.findall(r'(\d+)(\D)', text)))
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["E aaabcc\nD 3a1b2c\nE x\nD 12z\n", "E \nD \n"]
    for _ in range(4):
        lines = []
        for _ in range(rng.randint(3, 20)):
            t = ''.join(rng.choice("abAB") * rng.randint(1, 15) for _ in range(rng.randint(0, 12)))
            lines.append("E " + t)
            enc = ''.join(f"{len(list(g))}{c}" for c, g in itertools.groupby(t))
            lines.append("D " + enc)
        out.append("\n".join(lines) + "\n")
    return out
