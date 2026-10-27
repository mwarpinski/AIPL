"""The reference solution of 26_anagrams: it defines the expected outputs
(tests/), and is never shown to a model."""
import random
from collections import defaultdict

def solve(inp):
    g = defaultdict(set)
    for w in inp.split("\n")[:-1]:
        g["".join(sorted(w))].add(w)
    groups = sorted(sorted(s) for s in g.values() if len(s) >= 2)
    return ''.join(" ".join(s) + "\n" for s in groups)

def cases(rng):
    out = ["listen\nenlist\ngoogle\nsilent\nbanana\ntinsel\nlisten\n", "", "a\na\n"]
    for n in [50, 3000]:
        words = []
        for _ in range(n):
            w = "".join(rng.choice("abcde") for _ in range(rng.randint(1, 5)))
            words.append(w)
        out.append("\n".join(words) + "\n")
    return out
