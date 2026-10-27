"""The reference solution of 01_word_freq: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, re
from collections import Counter

def solve(inp):
    c = Counter(w.lower() for w in re.findall(r'[A-Za-z]+', inp))
    top = sorted(c.items(), key=lambda kv: (-kv[1], kv[0]))[:10]
    return ''.join(f"{w} {n}\n" for w, n in top)

def cases(rng):
    words = ["alpha", "beta", "Gamma", "delta", "eps", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu"]
    out = ["The cat and the hat.\nTHE END, and the bat!\n", "", "123 ... !!!\n", "one\n"]
    for _ in range(4):
        n = rng.randint(20, 400)
        seps = [" ", "  ", ", ", ".\n", "-", "\t", "; ", "42"]
        out.append(''.join(rng.choice(words) + rng.choice(seps) for _ in range(n)) + "\n")
    return out
