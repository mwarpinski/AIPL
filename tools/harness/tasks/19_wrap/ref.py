"""The reference solution of 19_wrap: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    lines = inp.split("\n")
    w = int(lines[0])
    paras, cur = [], []
    for l in lines[1:]:
        if l.strip() == "":
            if cur: paras.append(cur); cur = []
        else:
            cur += l.split()
    if cur: paras.append(cur)
    out = []
    for p in paras:
        line = ""
        for word in p:
            if line == "": line = word
            elif len(line) + 1 + len(word) <= w: line += " " + word
            else: out.append(line); line = word
        out.append(line)
        out.append("")
    if out: out.pop()
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["20\nThe quick brown fox jumps over\nthe lazy dog.\n\nA second paragraph.\n", "10\n", "10\nabcdefghijklmnop short\n"]
    words = ["a", "an", "the", "quick", "extraordinarily", "fox", "jumps", "over", "lazy", "dogs", "and", "cats", "run"]
    for w in [12, 40, 79]:
        paras = []
        for _ in range(rng.randint(1, 6)):
            lines = [" ".join(rng.choice(words) for _ in range(rng.randint(1, 12))) for _ in range(rng.randint(1, 5))]
            paras.append("\n".join(lines))
        out.append(f"{w}\n" + ("\n" + rng.choice(["", "   ", ""]) + "\n").join(paras) + "\n")
    return out
