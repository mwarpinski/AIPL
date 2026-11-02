#!/usr/bin/env python3
"""Test vectors for std/math (tests/test_math.rs): Python's answers.

    math_vectors.py gen DIR [--cases N] [--seed S]   write DIR/cases.txt (lines `OP ARG...`) and
                                                     DIR/expected.txt (one answer per line)
    math_vectors.py check DIR GOT                    compare the program's answers in GOT with
                                                     expected.txt; print each mismatch, exit 1 on any

Integer answers must match exactly, as must floor, ceil, trunc, round, min,
max, and abs on floats (compared as numbers, so a zero has no sign); powi is
compared to Python's x ** n within a relative 1e-12, since it multiplies
where Python calls libm's pow.
"""
import argparse
import math
import os
import random
import sys
from decimal import ROUND_HALF_UP, Decimal

MIN32, MAX32 = -(2**31), 2**31 - 1
MIN64, MAX64 = -(2**63), 2**63 - 1
FLOAT_OPS = {"floor", "ceil", "trunc", "round", "min_f64", "max_f64", "abs_f64", "powi"}


def ints(rng, lo, hi):
    """A number from lo..hi, often small or at an edge."""
    pick = rng.random()
    if pick < 0.25:
        return rng.randint(max(lo, -12), min(hi, 12))
    if pick < 0.40:
        return rng.choice([n for n in (lo, hi, 0, 1, -1, lo + 1, hi - 1, 2, -2) if lo <= n <= hi])
    if pick < 0.70:
        return rng.randint(max(lo, -1000), min(hi, 1000))
    return rng.randint(lo, hi)


def floats(rng):
    pick = rng.random()
    if pick < 0.2:
        return float(rng.randint(-50, 50))
    if pick < 0.4:
        return rng.randint(-50, 50) + 0.5
    if pick < 0.45:
        return rng.choice([0.49999999999999994, 0.5000000000000001, -0.49999999999999994, 1.5, -1.5, 2.5, -2.5, 4503599627370495.5, -4503599627370495.5, 4503599627370496.0, 1e15 + 0.5])
    if pick < 0.7:
        return rng.uniform(-1e6, 1e6)
    sign = rng.choice([-1, 1])
    return sign * 10 ** rng.uniform(-300, 300)


def fmt(x):
    return repr(x)


def case(rng):
    """(operation, arguments, expected answer as text) for one random case."""
    op = rng.choice(["min", "max", "abs", "sign", "clamp", "floor_div", "floor_mod", "gcd", "pow", "isqrt",
                     "min_i64", "max_i64", "abs_i64", "clamp_i64", "floor_div_i64", "floor_mod_i64", "gcd_i64", "pow_i64", "isqrt_i64",
                     "floor", "ceil", "trunc", "round", "min_f64", "max_f64", "abs_f64", "powi"])
    wide = op.endswith("_i64")
    lo, hi = (MIN64, MAX64) if wide else (MIN32, MAX32)
    base = op[:-4] if wide else op
    if base in ("min", "max") and op not in FLOAT_OPS:
        a, b = ints(rng, lo, hi), ints(rng, lo, hi)
        return op, [a, b], str((min if base == "min" else max)(a, b))
    if base == "abs":
        a = ints(rng, lo + 1, hi)
        return op, [a], str(abs(a))
    if op == "sign":
        a = ints(rng, lo, hi)
        return op, [a], str((a > 0) - (a < 0))
    if base == "clamp":
        a, b, c = ints(rng, lo, hi), ints(rng, lo, hi), ints(rng, lo, hi)
        b, c = min(b, c), max(b, c)
        return op, [a, b, c], str(min(max(a, b), c))
    if base in ("floor_div", "floor_mod"):
        a, b = ints(rng, lo, hi), ints(rng, lo, hi)
        if b == 0 or (a == lo and b == -1):
            return case(rng)
        return op, [a, b], str(a // b if base == "floor_div" else a % b)
    if base == "gcd":
        a, b = ints(rng, lo + 1, hi), ints(rng, lo + 1, hi)
        return op, [a, b], str(math.gcd(a, b))
    if base == "pow":
        x = rng.choice([rng.randint(-12, 12), rng.choice([2, -2, 3, 10, -10])])
        n = rng.randint(0, 40 if wide else 31)
        r = x**n
        if not lo <= r <= hi:
            return case(rng)
        return op, [x, n], str(r)
    if base == "isqrt":
        if rng.random() < 0.4:
            k = rng.randint(0, 3037000499 if wide else 46340)
            a = max(0, min(hi, k * k + rng.choice([-1, 0, 1])))
        else:
            a = ints(rng, 0, hi)
        return op, [a], str(math.isqrt(a))
    if op in ("floor", "ceil", "trunc", "round"):
        x = floats(rng)
        if op == "round":
            r = float(Decimal(x).to_integral_value(rounding=ROUND_HALF_UP))
        else:
            r = float({"floor": math.floor, "ceil": math.ceil, "trunc": math.trunc}[op](x))
        return op, [fmt(x)], fmt(r)
    if op in ("min_f64", "max_f64"):
        a, b = floats(rng), floats(rng)
        return op, [fmt(a), fmt(b)], fmt((min if op == "min_f64" else max)(a, b))
    if op == "abs_f64":
        x = floats(rng)
        return op, [fmt(x)], fmt(abs(x))
    if op == "powi":
        x = rng.choice([-1, 1]) * rng.uniform(0.5, 3.0)
        n = rng.randint(-20, 20)
        return op, [fmt(x), n], fmt(x**n)
    raise AssertionError(op)


def cmd_gen(args):
    rng = random.Random(args.seed)
    os.makedirs(args.dir, exist_ok=True)
    with open(os.path.join(args.dir, "cases.txt"), "w") as cases, open(os.path.join(args.dir, "expected.txt"), "w") as want:
        for _ in range(args.cases):
            op, a, expect = case(rng)
            cases.write(" ".join([op] + [str(x) for x in a]) + "\n")
            want.write(expect + "\n")


def cmd_check(args):
    cases = open(os.path.join(args.dir, "cases.txt")).read().split("\n")
    want = open(os.path.join(args.dir, "expected.txt")).read().split("\n")
    got = open(args.got).read().split("\n")
    bad = 0
    if len(got) != len(want):
        print(f"{len(got) - 1} answers for {len(want) - 1} cases")
        bad += 1
    for k, (c, w, g) in enumerate(zip(cases, want, got)):
        if not c:
            continue
        op = c.split()[0]
        if op in FLOAT_OPS:
            try:
                x, y = float(g), float(w)
            except ValueError:
                ok = False
            else:
                ok = math.isclose(x, y, rel_tol=1e-12, abs_tol=0.0) if op == "powi" else x == y
        else:
            ok = g == w
        if not ok:
            bad += 1
            if bad <= 20:
                print(f"case {k}: {c}: got {g!r}, Python says {w!r}")
    print(f"{len(want) - 1} cases, {bad} mismatches")
    sys.exit(1 if bad else 0)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("gen")
    s.add_argument("dir")
    s.add_argument("--cases", type=int, default=3000)
    s.add_argument("--seed", type=int, default=1)
    s = sub.add_parser("check")
    s.add_argument("dir")
    s.add_argument("got")
    args = p.parse_args()
    {"gen": cmd_gen, "check": cmd_check}[args.cmd](args)


if __name__ == "__main__":
    main()
