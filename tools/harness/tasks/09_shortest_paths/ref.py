"""The reference solution of 09_shortest_paths: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, heapq

def solve(inp):
    it = iter(inp.split())
    n, m = int(next(it)), int(next(it))
    adj = [[] for _ in range(n)]
    for _ in range(m):
        u, v, w = int(next(it)), int(next(it)), int(next(it))
        adj[u].append((v, w))
    dist = [None] * n
    pq = [(0, 0)]
    while pq:
        d, u = heapq.heappop(pq)
        if dist[u] is not None:
            continue
        dist[u] = d
        for v, w in adj[u]:
            if dist[v] is None:
                heapq.heappush(pq, (d + w, v))
    return ''.join(("inf" if d is None else str(d)) + "\n" for d in dist)

def cases(rng):
    out = ["4 4\n0 1 5\n0 2 1\n2 1 2\n1 3 1\n", "1 0\n", "3 1\n1 2 4\n"]
    for size in [(50, 200), (2000, 20000), (300, 400)]:
        n, m = size
        edges = [f"{rng.randrange(n)} {rng.randrange(n)} {rng.randint(0, 1000)}" for _ in range(m)]
        out.append(f"{n} {m}\n" + "\n".join(edges) + "\n")
    return out
