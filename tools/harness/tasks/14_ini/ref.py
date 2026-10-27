"""The reference solution of 14_ini: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    sec = ""; d = {}
    for line in inp.split("\n"):
        s = line.strip()
        if s == "" or s[0] in ";#":
            continue
        if s[0] == "[" and s[-1] == "]":
            sec = s[1:-1]
            continue
        k, v = s.split("=", 1)
        d[(sec, k.strip())] = v.strip()
    return ''.join(f"{s}.{k}={v}\n" for (s, k), v in sorted(d.items()))

def cases(rng):
    out = ["name = demo\n[server]\nport = 8080\nhost=localhost\n; a comment\n[client]\nretries = 3\n[server]\nport = 9090\n",
           "", "# only a comment\n", "[a]\nx = y = z\n"]
    secs = ["db", "web", "cache", "log"]; keys = ["host", "port", "user", "level", "size", "path"]
    for n in [30, 300]:
        lines = []
        for _ in range(n):
            r = rng.random()
            if r < 0.15: lines.append(f"[{rng.choice(secs)}]")
            elif r < 0.25: lines.append(rng.choice(["", "; note", "  # x = 1", "   "]))
            else: lines.append(f"{' ' * rng.randint(0, 2)}{rng.choice(keys)}{' ' * rng.randint(0, 2)}={' ' * rng.randint(0, 2)}{rng.choice(['1', 'on', 'a b', '/tmp', 'x=y'])}")
        out.append("\n".join(lines) + "\n")
    return out
