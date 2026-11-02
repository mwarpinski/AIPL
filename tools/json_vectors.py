#!/usr/bin/env python3
"""Documents, and the layouts Python's json module gives them, for
tests/test_showcase.rs (examples/jsonpp.aipl).

    json_vectors.py DIR [--cases N] [--seed S]

For each case K it writes DIR/K.json (the document, spaced and escaped at
random: indented or not, \\u escapes or raw characters), DIR/K.pretty (two
spaces a level), DIR/K.sorted4 (members in key order, four spaces), and
DIR/K.compact, each as jsonpp prints it: the text and a newline.

The numbers are ones both languages write alike: integers up to 10^15, and
multiples of 1/8 that are not whole (so Python never writes 3.0 where
JavaScript's rules write 3). Strings and keys come from a pool with quotes,
backslashes, controls, accents, CJK, and an emoji; keys are Unicode, so the
sort is by code point, which is UTF-8's byte order.
"""
import argparse
import json
import os
import random

STRINGS = ["", "plain", "with space", 'quo"te', "back\\slash", "tab\there", "new\nline", "é", "日本語",
           "\U0001F600 emoji", "\u0001ctl", "slash/", "\u007f", "a", "b", "B", "ab", "_", "10", "9", "z", "éa"]


def value(rng, depth):
    r = rng.random()
    if depth == 0 or r < 0.4:
        kind = rng.randrange(6)
        if kind == 0:
            return None
        if kind == 1:
            return rng.random() < 0.5
        if kind == 2:
            return rng.randint(-10**15, 10**15)
        if kind == 3:
            return rng.randint(-1000, 1000)
        if kind == 4:
            k = rng.randint(-5000, 5000)
            return k / 8 if k % 8 else k // 8
        return rng.choice(STRINGS)
    if r < 0.7:
        return [value(rng, depth - 1) for _ in range(rng.choice([0, 1, 2, 3, 5]))]
    return {rng.choice(STRINGS): value(rng, depth - 1) for _ in range(rng.choice([0, 1, 2, 4, 6]))}


def main():
    p = argparse.ArgumentParser()
    p.add_argument("dir")
    p.add_argument("--cases", type=int, default=40)
    p.add_argument("--seed", type=int, default=1)
    a = p.parse_args()
    os.makedirs(a.dir, exist_ok=True)
    rng = random.Random(a.seed)
    for k in range(a.cases):
        x = value(rng, rng.randint(1, 5))
        text = json.dumps(x, indent=rng.choice([None, 0, 1, 3, "\t"]), ensure_ascii=rng.random() < 0.5)
        out = {
            "json": text,
            "pretty": json.dumps(x, indent=2, ensure_ascii=False),
            "sorted4": json.dumps(x, indent=4, sort_keys=True, ensure_ascii=False),
            "compact": json.dumps(x, separators=(",", ":"), ensure_ascii=False),
        }
        for ext, s in out.items():
            with open(os.path.join(a.dir, f"{k}.{ext}"), "w", encoding="utf-8", newline="") as f:
                f.write(s if ext == "json" else s + "\n")


if __name__ == "__main__":
    main()
