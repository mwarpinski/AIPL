"""The reference solution of 04_rpn: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def ev(line):
    st = []
    if line == "":
        return "error"
    for t in line.split(" "):
        if t in "+-*/" and len(t) == 1:
            if len(st) < 2:
                return "error"
            b = st.pop(); a = st.pop()
            if t == '+': st.append(a + b)
            elif t == '-': st.append(a - b)
            elif t == '*': st.append(a * b)
            else:
                if b == 0:
                    return "error"
                q = abs(a) // abs(b)
                st.append(q if (a >= 0) == (b >= 0) else -q)
        else:
            st.append(int(t))
    return str(st[0]) if len(st) == 1 else "error"

def solve(inp):
    return ''.join(ev(l) + "\n" for l in inp.split("\n")[:-1])

def gen(rng, depth):
    if depth == 0 or rng.random() < 0.3:
        return [str(rng.randint(-50, 50))]
    return gen(rng, depth - 1) + gen(rng, depth - 1) + [rng.choice("+-*/")]

def cases(rng):
    out = ["3 4 + 2 *\n7 -2 /\n1 +\n4 0 /\n", "\n", "5\n", "1 2\n", "-7 2 /\n"]
    for _ in range(4):
        lines = [" ".join(gen(rng, rng.randint(1, 5))) for _ in range(rng.randint(5, 40))]
        out.append("\n".join(lines) + "\n")
    return out
