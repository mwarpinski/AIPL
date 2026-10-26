#!/usr/bin/env python3
"""Does aipl_src/codegen_opt.aipl change what programs do? For each program
with a main: compile it as usual (`aipl compile`) and with the optimizer's
own pipeline (`codegen_opt compile`: resolve, check, lower, optimize, then
codegen.aipl), run both, and compare standard output and exit status.
(The optimizer's output text is codegen.aipl's input, after enums are
numbers, not AIPL source, so it is compared by running what it compiles to.) Standard error is compared with positions taken out
(optimizing moves code, so line:col and call chains may differ), and only
for a program that fails (one that succeeds may print timings there).

    python3 tools/opt_diff.py [--jobs N] [PROGRAM.aipl ...]
    python3 tools/opt_diff.py --fuzz 300 [--seed 1]   # tools/run_fuzz.py's programs

Without programs: examples/, benchmarks/ (with a small size argument),
and tests/aipl/. Writes target/opt/ (the optimizer's wasm, each optimized
program) and prints one line per program that differs or fails, then a
summary. Build first: cargo build --release.
"""
import concurrent.futures as cf
import glob
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AIPL = os.path.join(ROOT, 'target/release/aipl')
OUT = os.path.join(ROOT, 'target/opt')
OPT = os.path.join(OUT, 'codegen_opt.wasm')
TIMEOUT = 60


def run(cmd, cwd):
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, timeout=TIMEOUT)
        return p.returncode, p.stdout.decode('utf-8', 'replace'), p.stderr.decode('utf-8', 'replace')
    except subprocess.TimeoutExpired:
        return 'timeout', '', ''


def scrub(err):
    """stderr without the toolchain's own lines and positions."""
    lines = [l for l in err.splitlines() if not l.startswith('[AIPL')]
    text = '\n'.join(lines)
    text = re.sub(r'^[^:\n]*\.wasm: ', '', text, flags=re.M)
    text = re.sub(r'[\w./-]*\.aipl:\d+:\d+', 'FILE:L:C', text)
    text = re.sub(r' at \d+:\d+', ' at L:C', text)
    text = re.sub(r'= \d{4,}', '= ADDR', text)
    text = re.sub(r'@ \d+', '@ ADDR', text)
    text = re.sub(r'\(\S+\.aipl:\d+:\d+\)', '(FILE:L:C)', text)
    return re.sub(r'\n  at .*', '', text).strip()


def vm_stdout(out):
    return '\n'.join(l for l in out.splitlines() if not l.startswith('[AIPL'))


def args_for(path):
    # benchmarks take a size; a small one keeps the VM quick
    return ['--', '10'] if '/benchmarks/' in path else []


def check(path):
    rel = os.path.relpath(path, ROOT)
    name = rel.replace('/', '_').replace('.aipl', '')
    cwd = os.path.dirname(path)
    args = args_for(path)
    plain = os.path.join(OUT, name + '.wasm')
    opt = os.path.join(OUT, name + '.opt.wasm')
    c = run([AIPL, 'compile', path, '-o', plain], ROOT)
    if c[0] != 0:
        return (rel, 'skip', 'does not compile: ' + (c[2] or c[1]).strip().splitlines()[-1][:120])
    o = run([AIPL, 'run', OPT, '--', 'compile', path, '-o', opt], ROOT)
    if o[0] != 0:
        return (rel, 'OPT FAILED', (o[2] or o[1]).strip()[-300:])
    a = run([AIPL, 'run', plain] + args, cwd)
    if a[0] == 'timeout':
        return (rel, 'skip', 'times out as written')
    b = run([AIPL, 'run', opt] + args, cwd)
    fa, fb = a[0] != 0, b[0] != 0
    addr = lambda out: re.sub(r'@ \d+', '@ ADDR', out)
    if (a[0], addr(a[1]), scrub(a[2]) if fa else '') != (b[0], addr(b[1]), scrub(b[2]) if fb else ''):
        return (rel, 'DIFFERS', f"status {a[0]} -> {b[0]}; stdout {a[1][-160:]!r} -> {b[1][-160:]!r}; stderr {scrub(a[2])[:160]!r} -> {scrub(b[2])[:160]!r}")
    return (rel, 'same', f"{os.path.getsize(plain)} -> {os.path.getsize(opt)} bytes")


def main():
    args = sys.argv[1:]
    jobs = 8
    if '--jobs' in args:
        i = args.index('--jobs')
        jobs = int(args[i + 1])
        del args[i:i + 2]
    os.makedirs(OUT, exist_ok=True)
    built = run([AIPL, 'compile', os.path.join(ROOT, 'aipl_src/codegen_opt.aipl'), '-o', OPT], ROOT)
    if built[0] != 0:
        sys.exit('cannot build the optimizer: ' + built[2])
    if '--fuzz' in args:
        # tools/run_fuzz.py's generated programs, case by case
        i = args.index('--fuzz')
        n, seed = int(args[i + 1]), 1
        if '--seed' in args:
            seed = int(args[args.index('--seed') + 1])
        fz = os.path.join(OUT, 'fuzz')
        os.makedirs(fz, exist_ok=True)
        progs = []
        for k in range(n):
            text = subprocess.run([sys.executable, os.path.join(ROOT, 'tools/run_fuzz.py'), '--seed', str(seed), '--show', str(k)],
                                  capture_output=True, text=True).stdout
            path = os.path.join(fz, f'case{k}.aipl')
            # the generator prints addresses so that backends agree on the heap's
            # layout; the optimizer may shrink the data, which moves the heap, so
            # addresses are left out here (everything else is compared)
            text = re.sub(r'\((ptr|arr)\.addr [^()]*\)', '(+ 0 0)', text)
            open(path, 'w').write(text)
            progs.append(path)
    elif args:
        progs = [os.path.abspath(a) for a in args]
    else:
        progs = sorted(glob.glob(os.path.join(ROOT, 'examples/*.aipl')) + glob.glob(os.path.join(ROOT, 'benchmarks/*/*.aipl'))
                       + glob.glob(os.path.join(ROOT, 'tests/aipl/**/*.aipl'), recursive=True))
        progs = [p for p in progs if '(fn main' in open(p).read()]
    counts = {}
    with cf.ThreadPoolExecutor(jobs) as ex:
        for rel, verdict, why in ex.map(check, progs):
            counts[verdict] = counts.get(verdict, 0) + 1
            if verdict != 'same':
                print(f"{verdict:28} {rel}: {why}")
    print(f"{len(progs)} programs: " + ', '.join(f"{v} {n}" for v, n in sorted(counts.items())))


main()
