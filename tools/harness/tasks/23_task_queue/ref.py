"""The reference solution of 23_task_queue: it defines the expected outputs
(tests/), and is never shown to a model."""
import random, heapq

def solve(inp):
    pq = []; out = []; seq = 0
    for l in inp.split("\n")[:-1]:
        p = l.split()
        if p[0] == "add":
            heapq.heappush(pq, (-int(p[2]), seq, p[1])); seq += 1
        else:
            out.append(heapq.heappop(pq)[2] if pq else "idle")
    out.append(f"left {len(pq)}")
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["add write 2\nadd test 5\nadd ship 5\nnext\nnext\nnext\nnext\nadd rest 1\n", "next\n", "add a -3\n"]
    for n in [50, 20000]:
        lines = []
        for i in range(n):
            if rng.random() < 0.6:
                lines.append(f"add t{i} {rng.randint(-100, 100)}")
            else:
                lines.append("next")
        out.append("\n".join(lines) + "\n")
    return out
