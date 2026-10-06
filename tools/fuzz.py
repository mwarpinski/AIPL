#!/usr/bin/env python3
"""Fuzzes the two toolchains against each other.

Each case mutates one of the repository's AIPL programs (deletes, inserts,
copies, or overwrites a few bytes, often with brackets and keywords) and
gives it to the Rust toolchain (`aipl verify`, then `aipl compile`) and to
the self-hosted compiler built natively (`aiplc`, from aipl_src/driver.aipl).
A case fails when either one crashes, hangs, or traps, or when they
disagree: one accepts what the other rejects, both accept but compile to
different bytes, or both reject with different messages. Programs `verify`
accepts must also compile.

    cargo build --release
    python3 tools/fuzz.py                  # 1000 cases, seed 1
    python3 tools/fuzz.py --cases 20000 --seed 7

Failing cases are kept under target/fuzz/ (the mutated file beside a
note.txt saying what went wrong); passing ones are deleted. The exit status
is the number of failures (capped at 100).
"""

import argparse
import concurrent.futures
import glob
import os
import random
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AIPL = os.path.join(ROOT, "target", "release", "aipl")
AIPLC = os.path.join(ROOT, "target", "fuzz-aiplc")
OUT = os.path.join(ROOT, "target", "fuzz")
TIMEOUT = 20

# fragments a mutation may insert: brackets, keywords, odd literals
PIECES = [b"(", b")", b"[", b"]", b'"', b":", b"->", b" ", b"0", b"-1", b"i32", b"(ptr", b"(arr i32)",
          b"call", b"let", b"x", b"1.5", b"9999999999", b"\\", b";", b".", b"(match ", b"(make ",
          b"(loop i 0 3 1 ", b"(return)", b"(break)", b"(import ", b"\xff", b"\xc2\xa0"]


def mutate(data, rng):
    b = bytearray(data)
    for _ in range(rng.randint(1, 4)):
        if not b:
            break
        i = rng.randrange(len(b))
        op = rng.random()
        if op < 0.3:
            del b[i:i + rng.randint(1, 8)]
        elif op < 0.6:
            b[i:i] = rng.choice(PIECES)
        elif op < 0.8:
            j = rng.randrange(len(b))
            b[i:i] = b[j:j + rng.randint(1, 30)]
        else:
            b[i] = rng.randrange(256)
    return bytes(b)


def run(cmd):
    """(exit status, combined output); status None on a timeout."""
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT, cwd=ROOT)
        return r.returncode, (r.stdout + r.stderr).decode("utf8", "replace")
    except subprocess.TimeoutExpired:
        return None, "timeout"


def message(out):
    """An error message without its file path and Rust's debug quoting."""
    # split on newlines only: a message may hold control characters
    lines = [l for l in out.split("\n") if l.strip()]
    line = lines[-1] if lines else ""
    if line.startswith('Error: "'):
        # undo Rust's debug quoting of the message
        esc = {"\\": "\\", '"': '"', "n": "\n", "t": "\t", "r": "\r", "0": "\0", "'": "'"}
        line = re.sub(r"\\u\{([0-9a-f]+)\}|\\(.)",
                      lambda m: chr(int(m.group(1), 16)) if m.group(1) else esc.get(m.group(2), m.group(0)),
                      line[len('Error: "'):-1])
    line = line.split(".aipl: ", 1)[-1]
    # the places searched for a missing module are listed in each toolchain's own path form
    return line.split(" found in ", 1)[0]


def check(case, sources, seed):
    rng = random.Random(seed * 1_000_003 + case)
    src = rng.choice(sources)
    with open(src, "rb") as f:
        data = mutate(f.read(), rng)
    d = os.path.join(OUT, f"{seed}_{case}")
    os.makedirs(d, exist_ok=True)
    # the modules beside it, which it may import
    for sib in glob.glob(os.path.join(os.path.dirname(src), "*.aipl")):
        if sib != src:
            shutil.copy(sib, d)
    path = os.path.join(d, os.path.basename(src))
    with open(path, "wb") as f:
        f.write(data)

    problems = []
    v, v_out = run([AIPL, "verify", path])
    a, a_out = run([AIPLC, path, os.path.join(d, "aiplc.wasm")])
    for name, status, out in (("aipl verify", v, v_out), ("aiplc", a, a_out)):
        if status is None:
            problems.append(f"{name} hangs")
        elif status not in (0, 1, 2) or "panicked" in out or "wasm trap" in out:
            problems.append(f"{name} crashed (exit {status}): {out[-300:]}")
    if not problems:
        if v == 0:
            c, c_out = run([AIPL, "compile", path, "-o", os.path.join(d, "rust.wasm")])
            if c != 0:
                problems.append(f"verify accepts but compile fails: {message(c_out)}")
            elif a != 0:
                problems.append(f"Rust accepts, aiplc rejects: {message(a_out)}")
            else:
                with open(os.path.join(d, "rust.wasm"), "rb") as f1, open(os.path.join(d, "aiplc.wasm"), "rb") as f2:
                    if f1.read() != f2.read():
                        problems.append("both accept, different bytes")
        elif a == 0:
            problems.append(f"Rust rejects ({message(v_out)}), aiplc accepts")
        elif message(v_out) != message(a_out):
            problems.append(f"different messages:\n  rust:  {message(v_out)}\n  aiplc: {message(a_out)}")
    if problems:
        with open(os.path.join(d, "note.txt"), "w") as f:
            f.write(f"from {os.path.relpath(src, ROOT)}\n" + "\n".join(problems) + "\n")
    else:
        shutil.rmtree(d)
    return path, problems


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--cases", type=int, default=1000)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    args = ap.parse_args()
    if not os.path.exists(AIPL):
        sys.exit("build first: cargo build --release")
    built = subprocess.run([AIPL, "compile", "--exe", "aipl_src/driver.aipl", "-o", AIPLC], capture_output=True, cwd=ROOT)
    if built.returncode != 0:
        sys.exit("could not build aiplc: " + built.stderr.decode())
    sources = [p for p in glob.glob(os.path.join(ROOT, "examples", "*.aipl"))
               + glob.glob(os.path.join(ROOT, "tests", "aipl", "*.aipl"))
               + glob.glob(os.path.join(ROOT, "benchmarks", "*", "*.aipl"))
               if os.path.getsize(p) < 20000]
    os.makedirs(OUT, exist_ok=True)
    failures = 0
    kinds = {}
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as ex:
        for path, problems in ex.map(lambda k: check(k, sources, args.seed), range(args.cases)):
            if problems:
                failures += 1
                kind = re.sub(r"[0-9]+", "N", problems[0].split(":")[0].split("(")[0]).strip()
                kinds[kind] = kinds.get(kind, 0) + 1
                if failures <= 20:
                    print(os.path.relpath(path, ROOT) + ": " + problems[0])
    for kind, n in sorted(kinds.items(), key=lambda kv: -kv[1]):
        print(f"  {n:5}  {kind}")
    print(f"{args.cases} cases, seed {args.seed}: {failures} failing (kept under target/fuzz/)")
    sys.exit(min(failures, 100))


if __name__ == "__main__":
    main()
