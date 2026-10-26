#!/usr/bin/env python3
"""Does the optimizer (aipl_src/codegen_opt.aipl) change what programs do?
For each program with a main: compile it without the optimizer (`aipl
compile --no-opt`, the Rust code generator) and with it (`aipl compile`,
the default: driver.aipl's compile_file_opt), run both, and compare
standard output and exit status.
For a program that fails, standard error too (one that succeeds may print
timings there): the message exactly, heap addresses aside, and the call
chain by chain_ok's rule (an inlined function has no frame).

    python3 tools/opt_diff.py [--jobs N] [PROGRAM.aipl ...]
    python3 tools/opt_diff.py --fuzz 300 [--seed 1]   # tools/run_fuzz.py's programs

Without programs: examples/, benchmarks/ (with a small size argument),
and tests/aipl/. Writes target/opt/ (each program, both ways) and prints one line per program that differs or fails, then a
summary; the exit status is 1 if any differs or fails to optimize. Build
first: cargo build --release. CI runs it on the repository's programs and on
500 generated ones (.github/workflows/ci.yml); the optimizer's own tests are
a group of aipl_src/test_suite.aipl.
"""
import concurrent.futures as cf
import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AIPL = os.path.join(ROOT, 'target/release/aipl')
OUT = os.path.join(ROOT, 'target/opt')
TIMEOUT = 60


def run(cmd, cwd, stdin=b''):
    try:
        p = subprocess.run(cmd, cwd=cwd, input=stdin, capture_output=True, timeout=TIMEOUT)
        return p.returncode, p.stdout.decode('utf-8', 'replace'), p.stderr.decode('utf-8', 'replace')
    except subprocess.TimeoutExpired:
        return 'timeout', '', ''


def split_err(err):
    """stderr as (message, call chain): the toolchain's own lines left out,
    the module's file name and heap addresses (the optimizer shrinks the
    data, which moves the heap) taken out of the message."""
    lines = [l for l in err.splitlines() if not l.startswith('[AIPL')]
    chain = [l.strip() for l in lines if l.startswith('  at ') or l.startswith('  ... ')]
    text = '\n'.join(l for l in lines if not (l.startswith('  at ') or l.startswith('  ... ')))
    text = re.sub(r'^[^:\n]*\.wasm: ', '', text, flags=re.M)
    text = re.sub(r'= \d{4,}', '= ADDR', text)
    text = re.sub(r'@ \d+', '@ ADDR', text)
    return text.strip(), chain


def chain_ok(plain, opt):
    """The optimized call chain is the plain one less whole frames (a function
    inlined into its caller has none, and a trap in it is reported at the
    call), every kept frame the same, position and all, and the outermost
    frame kept."""
    if bool(plain) != bool(opt) or (plain and plain[-1] != opt[-1]):
        return False
    it = iter(plain)
    return all(any(f == g for g in it) for f in opt)


def vm_stdout(out):
    return '\n'.join(l for l in out.splitlines() if not l.startswith('[AIPL'))


def args_for(path):
    # benchmarks take a size; a small one keeps the VM quick
    return ['--', '10'] if '/benchmarks/' in path else []


def stdin_for(path):
    # a benchmark's own input file (benchmarks/NAME/stdin), else none
    p = os.path.join(os.path.dirname(path), 'stdin')
    return open(p, 'rb').read() if '/benchmarks/' in path and os.path.exists(p) else b''


def run_in_copy(wasm, path, args):
    """Runs wasm in a fresh copy of the program's directory (all a program
    can reach at run time): the plain and optimized runs cannot see each
    other's files, and nothing is left in the repository."""
    with tempfile.TemporaryDirectory(dir=OUT) as tmp:
        cwd = os.path.join(tmp, 'd')
        shutil.copytree(os.path.dirname(path), cwd)
        return run([AIPL, 'run', wasm] + args, cwd, stdin_for(path))


def check(path):
    rel = os.path.relpath(path, ROOT)
    name = rel.replace('/', '_').replace('.aipl', '')
    args = args_for(path)
    plain = os.path.join(OUT, name + '.wasm')
    opt = os.path.join(OUT, name + '.opt.wasm')
    c = run([AIPL, 'compile', '--no-opt', path, '-o', plain], ROOT)
    if c[0] != 0:
        return (rel, 'skip', 'does not compile: ' + (c[2] or c[1]).strip().splitlines()[-1][:120])
    o = run([AIPL, 'compile', path, '-o', opt], ROOT)
    if o[0] != 0:
        return (rel, 'OPT FAILED', (o[2] or o[1]).strip()[-300:])
    a = run_in_copy(plain, path, args)
    if a[0] == 'timeout':
        return (rel, 'skip', 'times out as written')
    b = run_in_copy(opt, path, args)
    addr = lambda out: re.sub(r'@ \d+', '@ ADDR', out)
    if (a[0], addr(a[1])) != (b[0], addr(b[1])):
        return (rel, 'DIFFERS', f"status {a[0]} -> {b[0]}; stdout {a[1][-160:]!r} -> {b[1][-160:]!r}")
    if a[0] != 0:
        (ma, ca), (mb, cb) = split_err(a[2]), split_err(b[2])
        if ma != mb:
            return (rel, 'DIFFERS', f"stderr {ma[-200:]!r} -> {mb[-200:]!r}")
        if not chain_ok(ca, cb):
            return (rel, 'CHAIN DIFFERS', f"{ca} -> {cb}")
    return (rel, 'same' if a[0] == 0 else 'same failure', f"{os.path.getsize(plain)} -> {os.path.getsize(opt)} bytes")


def main():
    args = sys.argv[1:]
    jobs = 8
    if '--jobs' in args:
        i = args.index('--jobs')
        jobs = int(args[i + 1])
        del args[i:i + 2]
    os.makedirs(OUT, exist_ok=True)
    # one compile first, so the parallel ones find the optimizer in the module
    # cache (src/selfhost.rs) rather than each compiling it
    run([AIPL, 'compile', os.path.join(ROOT, 'examples/math_core.aipl'), '-o', os.path.join(OUT, 'warm.wasm')], ROOT)
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
            # each in its own directory, which run_in_copy copies
            os.makedirs(os.path.join(fz, f'case{k}'), exist_ok=True)
            path = os.path.join(fz, f'case{k}', f'case{k}.aipl')
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
            if not verdict.startswith('same'):
                print(f"{verdict:28} {rel}: {why}")
    print(f"{len(progs)} programs: " + ', '.join(f"{v} {n}" for v, n in sorted(counts.items())))
    sys.exit(0 if all(v.startswith('same') or v == 'skip' for v in counts) else 1)


main()
