#!/usr/bin/env python3
"""Benchmarks (benchmarks/<name>/): builds each program four ways and times
them on the same input, after checking every version prints the same
output as the C reference.

    python3 tools/bench.py              # every benchmark
    python3 tools/bench.py spigot       # one
    python3 tools/bench.py --quick      # each benchmark's test size only

Ways: AIPL native (`aipl compile --exe`), AIPL wasm under the launcher
(`--target wasm`, wasmtime/Cranelift), C (`gcc -O2`), Python 3. Each time is
the best of RUNS runs, wall clock, process start included. Needs a release
build (`cargo build --release`) and gcc; prints a Markdown table.
"""
import os, subprocess, sys, time, shutil

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCH = os.path.join(ROOT, "benchmarks")
OUT = os.path.join(ROOT, "target", "bench")
AIPL = os.path.join(ROOT, "target", "release", "aipl")
RUNS = 3

def sizes(name):
    """(test args, benchmark args) from the benchmark's files."""
    d = os.path.join(BENCH, name)
    test = open(os.path.join(d, "test_args")).read().split()
    bench = open(os.path.join(d, "bench_args")).read().split()
    return test, bench

def build(name):
    src = os.path.join(BENCH, name)
    os.makedirs(OUT, exist_ok=True)
    exe = {}
    for target in ["native", "wasm"]:
        exe[target] = os.path.join(OUT, f"{name}_{target}")
        subprocess.run([AIPL, "compile", "--exe", f"--target={target}", os.path.join(src, f"{name}.aipl"), "-o", exe[target]],
                       check=True, capture_output=True, env=dict(os.environ, AIPL_RUNNER=os.path.join(ROOT, "target", "release", "aipl-run")))
    exe["c"] = os.path.join(OUT, f"{name}_c")
    subprocess.run(["gcc", "-O2", "-o", exe["c"], os.path.join(src, f"{name}.c"), "-lm"], check=True)
    return {
        "AIPL native": [exe["native"]],
        "AIPL wasm (wasmtime)": [exe["wasm"]],
        "C (gcc -O2)": [exe["c"]],
        "Python 3": [sys.executable, os.path.join(src, f"{name}.py")],
    }

def run(cmd, args, stdin):
    t = time.perf_counter()
    p = subprocess.run(cmd + args, input=stdin, capture_output=True, cwd=os.path.join(BENCH))
    return time.perf_counter() - t, p

def stdin_for(name):
    p = os.path.join(BENCH, name, "stdin")
    return open(p, "rb").read() if os.path.exists(p) else b""

def main():
    quick = "--quick" in sys.argv
    names = [a for a in sys.argv[1:] if not a.startswith("--")] or sorted(
        n for n in os.listdir(BENCH) if os.path.isdir(os.path.join(BENCH, n)))
    rows = []
    for name in names:
        test, bench = sizes(name)
        args = test if quick else bench
        cmds = build(name)
        stdin = stdin_for(name)
        reference = None
        times = {}
        for way, cmd in cmds.items():
            best = None
            for _ in range(RUNS):
                t, p = run(cmd, args, stdin)
                if p.returncode != 0:
                    sys.exit(f"{name} / {way}: exit {p.returncode}: {p.stderr.decode()[:300]}")
                if reference is None:
                    reference = p.stdout
                elif p.stdout != reference:
                    sys.exit(f"{name} / {way}: output differs from the first version's")
                best = t if best is None else min(best, t)
            times[way] = best
        rows.append((name, " ".join(args), times))
    ways = list(rows[0][2].keys())
    print("| Benchmark | Input | " + " | ".join(ways) + " |")
    print("|---|---|" + "---|" * len(ways))
    for name, args, times in rows:
        c = times["C (gcc -O2)"]
        cells = [f"{times[w]:.3f} s ({times[w] / c:.1f}x)" for w in ways]
        print(f"| {name} | {args} | " + " | ".join(cells) + " |")

if __name__ == "__main__":
    main()
