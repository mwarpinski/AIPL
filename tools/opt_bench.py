#!/usr/bin/env python3
"""Does aipl_src/codegen_opt.aipl make programs faster? Each benchmark in
benchmarks/ at its benchmark size (its bench_args, and its stdin_cmd's
input, as tools/bench.py runs it), built four ways: native and wasm,
without the optimizer (`aipl compile --no-opt`) and with it (`aipl
compile`, the default). All four must print the same; the best of RUNS
wall-clock times is reported, and optimized over plain (below 1.0 is
faster). Run on a quiet machine.

    python3 tools/opt_bench.py [NAME ...]
"""
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCH = os.path.join(ROOT, 'benchmarks')
OUT = os.path.join(ROOT, 'target', 'opt_bench')
AIPL = os.path.join(ROOT, 'target', 'release', 'aipl')
RUNNER = os.path.join(ROOT, 'target', 'release', 'aipl-run')
RUNS = 5


def sh(cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, **kw)


def stdin_for(name):
    d = os.path.join(BENCH, name)
    cmd = os.path.join(d, 'stdin_cmd')
    if os.path.exists(cmd):
        script, *args = open(cmd).read().split()
        return sh([sys.executable, os.path.join(d, script)] + args).stdout
    p = os.path.join(d, 'stdin')
    return open(p, 'rb').read() if os.path.exists(p) else b''


def best(cmd, stdin):
    times, out = [], None
    for _ in range(RUNS):
        t = time.perf_counter()
        p = subprocess.run(cmd, input=stdin, capture_output=True, cwd=BENCH)
        times.append(time.perf_counter() - t)
        if p.returncode != 0:
            raise SystemExit(f"{cmd[0]} failed: {p.stderr.decode()[-300:]}")
        out = p.stdout
    return min(times), out


def main():
    os.makedirs(OUT, exist_ok=True)
    names = sys.argv[1:] or sorted(n for n in os.listdir(BENCH) if os.path.exists(os.path.join(BENCH, n, n + '.aipl')))
    print(f"{'benchmark':14} {'native':>8} {'native+opt':>11} {'ratio':>6}   {'wasm':>8} {'wasm+opt':>9} {'ratio':>6}")
    for name in names:
        src = os.path.join(BENCH, name, name + '.aipl')
        args = open(os.path.join(BENCH, name, 'bench_args')).read().split()
        stdin = stdin_for(name)
        b = {k: os.path.join(OUT, f'{name}.{k}') for k in ['native', 'native_opt', 'wasm', 'wasm_opt']}
        sh([AIPL, 'compile', '--no-opt', '--exe', '--target=native', src, '-o', b['native']])
        sh([AIPL, 'compile', '--no-opt', src, '-o', b['wasm']])
        sh([AIPL, 'compile', '--exe', '--target=native', src, '-o', b['native_opt']])
        sh([AIPL, 'compile', src, '-o', b['wasm_opt']])
        r = {}
        for k in ['native', 'native_opt']:
            r[k] = best([b[k]] + args, stdin)
        for k in ['wasm', 'wasm_opt']:
            r[k] = best([RUNNER, b[k]] + args, stdin)
        outs = {v[1] for v in r.values()}
        same = 'same output' if len(outs) == 1 else 'OUTPUTS DIFFER'
        n, no, w, wo = (r[k][0] for k in ['native', 'native_opt', 'wasm', 'wasm_opt'])
        print(f"{name:14} {n:8.3f} {no:11.3f} {no / n:6.2f}   {w:8.3f} {wo:9.3f} {wo / w:6.2f}   {same}")


main()
