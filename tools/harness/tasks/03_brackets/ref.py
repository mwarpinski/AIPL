"""The reference solution of 03_brackets: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def check(line):
    stack = []
    pairs = {')': '(', ']': '[', '}': '{'}
    for i, ch in enumerate(line, 1):
        if ch in "([{":
            stack.append(ch)
        elif ch in pairs:
            if not stack or stack[-1] != pairs[ch]:
                return f"mismatch at {i}"
            stack.pop()
    return "unclosed" if stack else "ok"

def solve(inp):
    return ''.join(check(l) + "\n" for l in inp.split("\n")[:-1])

def cases(rng):
    out = ["(a[b]{c})\n([)]\n((x)\n}\n", "\n", "no brackets here\n"]
    for _ in range(4):
        lines = []
        for _ in range(rng.randint(5, 40)):
            s = ''.join(rng.choice("()[]{}ab ") for _ in range(rng.randint(0, 30)))
            if rng.random() < 0.4:
                depth = rng.randint(1, 8)
                s = ''.join(rng.choice("([{") for _ in range(depth))
                s = s + ''.join({'(': ')', '[': ']', '{': '}'}[c] for c in reversed(s))
            lines.append(s)
        out.append("\n".join(lines) + "\n")
    return out
