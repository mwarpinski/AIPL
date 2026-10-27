"""The reference solution of 17_bst: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    vals = [int(x) for x in inp.split()]
    root = None
    left, right, key = {}, {}, {}
    nodes = 0
    for v in vals:
        if root is None:
            root = 0; key[0] = v; nodes = 1
            continue
        cur = root
        while True:
            if v == key[cur]:
                break
            side = left if v < key[cur] else right
            if cur in side:
                cur = side[cur]
            else:
                side[cur] = nodes; key[nodes] = v; nodes += 1
                break
    ino, pre = [], []
    height = 0
    if root is not None:
        stack = [(root, False, 1)]
        while stack:
            n, seen, d = stack.pop()
            height = max(height, d)
            if seen:
                ino.append(key[n])
                if n in right: stack.append((right[n], False, d + 1))
            else:
                stack.append((n, True, d))
                if n in left: stack.append((left[n], False, d + 1))
        stack = [root]
        while stack:
            n = stack.pop(); pre.append(key[n])
            if n in right: stack.append(right[n])
            if n in left: stack.append(left[n])
    return " ".join(map(str, ino)) + f"\nheight {height}\n" + " ".join(map(str, pre)) + "\n"

def cases(rng):
    out = ["5 3 8 1 4 5 9\n", "", "7\n", " ".join(map(str, range(1, 2001))) + "\n"]
    for n in [50, 3000]:
        out.append(" ".join(str(rng.randint(-10000, 10000)) for _ in range(n)) + "\n")
    return out
