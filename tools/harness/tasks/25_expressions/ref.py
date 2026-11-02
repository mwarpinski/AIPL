"""The reference solution of 25_expressions: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

class Bad(Exception):
    pass

def evaluate(s):
    toks = []
    i = 0
    while i < len(s):
        c = s[i]
        if c == " ": i += 1
        elif c.isdigit():
            j = i
            while j < len(s) and s[j].isdigit(): j += 1
            toks.append(int(s[i:j])); i = j
        elif c in "+-*/()": toks.append(c); i += 1
        else: raise Bad
    pos = 0
    def peek():
        return toks[pos] if pos < len(toks) else None
    def take():
        nonlocal pos
        t = peek(); pos += 1; return t
    def expr():
        v = term()
        while peek() in ("+", "-"):
            op = take(); r = term()
            v = v + r if op == "+" else v - r
        return v
    def term():
        v = unary()
        while peek() in ("*", "/"):
            op = take(); r = unary()
            if op == "*": v = v * r
            else:
                if r == 0: raise Bad
                q = abs(v) // abs(r)
                v = q if (v >= 0) == (r >= 0) else -q
        return v
    def unary():
        if peek() == "-":
            take(); return -unary()
        return atom()
    def atom():
        t = take()
        if isinstance(t, int): return t
        if t == "(":
            v = expr()
            if take() != ")": raise Bad
            return v
        raise Bad
    v = expr()
    if pos != len(toks): raise Bad
    return v

def solve(inp):
    out = []
    for l in inp.split("\n")[:-1]:
        try: out.append(str(evaluate(l)))
        except Bad: out.append("error")
    return ''.join(o + "\n" for o in out)

def gen(rng, d):
    if d == 0 or rng.random() < 0.3:
        return str(rng.randint(0, 99))
    r = rng.random()
    if r < 0.15: return "(" + gen(rng, d - 1) + ")"
    if r < 0.25: return "-" + gen(rng, d - 1)
    return gen(rng, d - 1) + rng.choice([" + ", "-", " * ", " / ", "*"]) + gen(rng, d - 1)

def cases(rng):
    out = ["1 + 2 * 3\n(1 + 2) * 3\n-7 / 2\n8 - 3 - 2\n2 * (3\n1 / 0\n", "\n", "42\n", "()\n1 +\n+1\n((7))\n"]
    for _ in range(3):
        out.append("".join(gen(rng, rng.randint(1, 6)) + "\n" for _ in range(rng.randint(5, 50))))
    return out
