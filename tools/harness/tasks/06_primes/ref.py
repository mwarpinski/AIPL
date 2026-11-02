"""The reference solution of 06_primes: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    ns = [int(l) for l in inp.split("\n")[:-1]]
    top = max(ns + [2])
    sieve = bytearray([1]) * (top + 1)
    sieve[0] = sieve[1] = 0
    for i in range(2, int(top ** 0.5) + 1):
        if sieve[i]:
            sieve[i * i::i] = bytearray(len(range(i * i, top + 1, i)))
    count = [0] * (top + 1); last = [0] * (top + 1)
    c = 0; l = 0
    for i in range(top + 1):
        if sieve[i]:
            c += 1; l = i
        count[i] = c; last[i] = l
    return ''.join(f"{count[n]} {last[n]}\n" for n in ns)

def cases(rng):
    out = ["10\n1\n2\n", "0\n", "2000000\n"]
    for _ in range(3):
        out.append("".join(f"{rng.randint(0, 2000000)}\n" for _ in range(rng.randint(1, 10))))
    return out
