#!/usr/bin/env python3
"""The expected transcript of tests/aipl/bigint_ops.aipl, computed with
Python's own integers: the same xorshift32 random walk over four registers.
tests/test_std.rs compares the AIPL program's output against the FNV-1a
hash printed by --hash (so no large file is checked in).

    python3 tools/bigint_vectors.py          # the transcript
    python3 tools/bigint_vectors.py --hash   # lines and FNV-1a 64 of it
"""
import sys

x = 123456789
def nxt():
    global x
    x ^= (x << 13) & 0xFFFFFFFF
    x ^= x >> 17
    x ^= (x << 5) & 0xFFFFFFFF
    return x >> 1

def mult():
    k = nxt() % 4
    v = 1 + (nxt() % 3 if k == 0 else nxt() % 1000 if k == 1 else nxt() % 2147483646)
    return -v if nxt() % 2 == 0 else v

def div_trunc(a, d):
    q = abs(a) // d * (-1 if a < 0 else 1)
    return q, a - q * d

def limbs(a): return (abs(a).bit_length() + 31) // 32

def cmp(a, b): return (a > b) - (a < b)

regs = [mult() for _ in range(4)]
lines = []
for _ in range(4000):
    op = nxt() % 10
    i = nxt() % 4
    j = nxt() % 4
    rem = 0
    if op == 0: regs[i] += regs[j]
    elif op == 1: regs[i] -= regs[j]
    elif op == 2: regs[i] += regs[j] * mult()
    elif op == 3: regs[i] -= regs[j] * mult()
    elif op == 5: regs[i], rem = div_trunc(regs[i], abs(mult()))
    elif op == 6: regs[i] = regs[j]
    elif op == 9:
        if regs[j] != 0:
            m = abs(mult())
            den = abs(regs[j])
            part = abs(regs[i])
            while limbs(part) >= limbs(den):
                part = part // 2147483647
            if nxt() % 2 == 0:
                part = den - 1 - part
            regs[i], rem = part, m
    else: regs[i] *= mult()
    while limbs(regs[i]) > 40:
        regs[i] = div_trunc(regs[i], 2147483647)[0]
    if regs[i] == 0 and nxt() % 8 != 0:
        regs[i] = mult()
    lines.append(f"{op} {regs[i]} {cmp(regs[i], regs[j])} {cmp(regs[i], 0)} {rem}\n")

text = "".join(lines)
if "--hash" in sys.argv:
    h = 0xcbf29ce484222325
    for b in text.encode():
        h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    print(len(lines), f"{h:#018x}", len(text))
else:
    sys.stdout.write(text)
