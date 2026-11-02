"""The reference solution of 15_base64: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, base64

def solve(inp):
    return ''.join(base64.b64encode(l.encode()).decode() + "\n" for l in inp.split("\n")[:-1])

def cases(rng):
    out = ["Man\nMa\nhello, world\n", "\n", "M\n"]
    for _ in range(3):
        lines = ["".join(chr(rng.randint(32, 126)) for _ in range(rng.randint(0, 120))) for _ in range(rng.randint(5, 60))]
        out.append("\n".join(lines) + "\n")
    return out
