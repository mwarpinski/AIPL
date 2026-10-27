"""The reference solution of 13_life: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    lines = inp.split("\n")
    g = int(lines[0])
    grid = [list(l) for l in lines[1:] if l != ""]
    h, w = len(grid), len(grid[0])
    for _ in range(g):
        new = [["."] * w for _ in range(h)]
        for y in range(h):
            for x in range(w):
                n = sum(grid[y + dy][x + dx] == "#" for dy in (-1, 0, 1) for dx in (-1, 0, 1)
                        if (dy or dx) and 0 <= y + dy < h and 0 <= x + dx < w)
                if n == 3 or (n == 2 and grid[y][x] == "#"):
                    new[y][x] = "#"
        grid = new
    return ''.join(''.join(r) + "\n" for r in grid)

def cases(rng):
    out = ["1\n.....\n..#..\n..#..\n..#..\n.....\n", "0\n#\n", "5\n#\n"]
    for g, h, w in [(10, 10, 12), (60, 30, 40), (200, 40, 60)]:
        rows = ["".join("#" if rng.random() < 0.35 else "." for _ in range(w)) for _ in range(h)]
        out.append(f"{g}\n" + "\n".join(rows) + "\n")
    return out
