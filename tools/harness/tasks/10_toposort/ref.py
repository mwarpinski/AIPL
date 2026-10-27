"""The reference solution of 10_toposort: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, heapq

def solve(inp):
    it = iter(inp.split())
    n, m = int(next(it)), int(next(it))
    adj = [[] for _ in range(n)]
    indeg = [0] * n
    for _ in range(m):
        a, b = int(next(it)), int(next(it))
        adj[a].append(b); indeg[b] += 1
    pq = [i for i in range(n) if indeg[i] == 0]
    heapq.heapify(pq)
    order = []
    while pq:
        u = heapq.heappop(pq)
        order.append(u)
        for v in adj[u]:
            indeg[v] -= 1
            if indeg[v] == 0:
                heapq.heappush(pq, v)
    if len(order) < n:
        return "cycle\n"
    return " ".join(map(str, order)) + "\n"

def cases(rng):
    out = ["4 3\n2 0\n1 0\n3 1\n", "3 3\n0 1\n1 2\n2 0\n", "1 0\n", "5 0\n"]
    for n, m in [(100, 300), (5000, 50000)]:
        perm = list(range(n)); rng.shuffle(perm)
        rules = []
        for _ in range(m):
            i, j = sorted(rng.sample(range(n), 2))
            rules.append(f"{perm[i]} {perm[j]}")
        out.append(f"{n} {m}\n" + "\n".join(rules) + "\n")
    return out
