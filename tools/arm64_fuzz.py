#!/usr/bin/env python3
"""Fuzzes the AArch64 backend against the x86-64 one.

The cases are tools/run_fuzz.py's generated programs (well typed and terminating by construction). Each is built
with `aipl compile --no-opt --exe` for x86-64 and for AArch64 (the same wasm, so the heap's addresses, which the
programs print, agree), the x86-64 executable runs here, and all the AArch64 ones run in one `linux/arm64`
container on HOST, an Apple Silicon Mac with Docker (as tools/arm64_remote.sh needs). A case fails when the
output, the exit status, or a trap's message and call chain differ (the program's own name aside).

    cargo build --release
    python3 tools/arm64_fuzz.py HOST                  # 200 cases, seed 1
    python3 tools/arm64_fuzz.py HOST --cases 2000 --seed 7

Failing cases are kept under target/arm64_fuzz/ (prog.aipl beside what each side printed); passing ones are
deleted. The exit status is the number of failures (capped at 100).
"""

import argparse
import concurrent.futures
import os
import random
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tools"))
import run_fuzz  # noqa: E402  (the generator)

AIPL = os.path.join(ROOT, "target", "release", "aipl")
OUT = os.path.join(ROOT, "target", "arm64_fuzz")
TIMEOUT = 30


def run(cmd, cwd, timeout=TIMEOUT):
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, timeout=timeout, stdin=subprocess.DEVNULL)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return None, b"", b""


def build(case, seed):
    """Writes the program and both executables; returns (directory, problem or None, x86-64's (status, out, err))."""
    rng = random.Random(seed * 1_000_003 + case)
    d = os.path.join(OUT, f"{seed}_{case}")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "prog.aipl"), "w") as f:
        f.write(run_fuzz.Gen(rng).program())
    for target, name in (("native", "x86"), ("arm64", "arm")):
        status, out, err = run([AIPL, "compile", "--no-opt", "--exe", "--target", target, "prog.aipl", "-o", name], d)
        if status != 0:
            return d, f"{target} build fails: {(out + err).decode(errors='replace').strip()[-300:]}", None
    return d, None, run(["./x86"], d)


# The bits of the default NaN, which an invalid operation (0/0, inf - inf) gives: x86-64's has the sign bit
# set, AArch64's does not, and wasm leaves it open. A program that prints those bits (reinterpret) prints
# one or the other, and a line that differs only so is not a difference.
NANS = ((b"-2251799813685248", b"9221120237041090560"), (b"-4194304", b"2143289344"))


def same_output(a, b):
    if a == b:
        return True
    la, lb = a.split(b"\n"), b.split(b"\n")
    if len(la) != len(lb):
        return False
    for x, y in zip(la, lb):
        if x == y:
            continue
        if not any(x.replace(neg, pos) == y.replace(neg, pos) and neg in (x + y) for neg, pos in NANS):
            return False
    return True


def normalized(err):
    """A trap's text without the program's own name, which differs (the first words up to ': ')."""
    text = err.decode(errors="replace")
    head, sep, rest = text.partition(": ")
    return rest if sep and "\n" not in head else text


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("host")
    ap.add_argument("--cases", type=int, default=200)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    args = ap.parse_args()
    if not os.path.exists(AIPL):
        sys.exit("build first: cargo build --release")
    # only this seed's earlier cases go: the kept ones of another seed stay
    os.makedirs(OUT, exist_ok=True)
    for old in os.listdir(OUT):
        if old.startswith(f"{args.seed}_"):
            shutil.rmtree(os.path.join(OUT, old), ignore_errors=True)
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as ex:
        built = list(ex.map(lambda k: build(k, args.seed), range(args.cases)))

    # the AArch64 executables, as one archive, run in one container
    work = tempfile.mkdtemp()
    names = {}
    with tarfile.open(os.path.join(work, "arm.tar"), "w") as tar:
        for k, (d, problem, _) in enumerate(built):
            if problem is None:
                tar.add(os.path.join(d, "arm"), arcname=f"p{k}")
                names[k] = f"p{k}"
    script = (
        "export PATH=/opt/homebrew/bin:/usr/local/bin:$PATH; d=$(mktemp -d ~/aipl-arm-fuzz-XXXXXX); cd $d; "
        "tar x; chmod +x p*; mkdir out; "
        "docker run --rm --platform linux/arm64 -v $d:/w -w /w/run alpine sh -c "
        "'mkdir -p /w/run; for f in /w/p*; do n=$(basename $f); timeout 30 $f > /w/out/$n.out 2> /w/out/$n.err < /dev/null; echo $? > /w/out/$n.st; done'; "
        "tar cf - out; cd ..; rm -rf $d"
    )
    with open(os.path.join(work, "arm.tar"), "rb") as f:
        r = subprocess.run(["ssh", "-o", "BatchMode=yes", args.host, script], stdin=f, capture_output=True)
    if r.returncode != 0 and not r.stdout:
        sys.exit("the run on the host failed: " + r.stderr.decode(errors="replace")[-400:])
    with open(os.path.join(work, "out.tar"), "wb") as f:
        f.write(r.stdout)
    arm = {}
    with tarfile.open(os.path.join(work, "out.tar")) as tar:
        for m in tar.getmembers():
            if m.isfile():
                arm[os.path.basename(m.name)] = tar.extractfile(m).read()
    shutil.rmtree(work)

    ran = sum(1 for n in names.values() if n + ".st" in arm)
    spoke = sum(1 for n in names.values() if arm.get(n + ".out"))
    trapped = sum(1 for n in names.values() if arm.get(n + ".st", b"0").strip() == b"134")
    print(f"{len(built)} built, {ran} run on {args.host}, {spoke} printed something, {trapped} trapped (status 134)")
    failures = 0
    kinds = {}
    for k, (d, problem, x86) in enumerate(built):
        problems = [problem] if problem else []
        if not problem:
            n = names[k]
            a_status = int(arm.get(n + ".st", b"-1").strip() or -1)
            a_out, a_err = arm.get(n + ".out", b""), arm.get(n + ".err", b"")
            x_status, x_out, x_err = x86
            if x_status is None:
                problems.append("x86-64 hangs")
            elif a_status == 124:
                problems.append("AArch64 hangs")
            else:
                if a_status != x_status:
                    problems.append(f"exit status differs: AArch64 {a_status}, x86-64 {x_status}")
                if not same_output(a_out, x_out):
                    problems.append("output differs")
                if normalized(a_err) != normalized(x_err):
                    problems.append("trap message or call chain differs")
            if problems:
                with open(os.path.join(d, "arm.out"), "wb") as f:
                    f.write(a_out)
                with open(os.path.join(d, "arm.err"), "wb") as f:
                    f.write(a_err)
                if x_status is not None:
                    with open(os.path.join(d, "x86.out"), "wb") as f:
                        f.write(x_out)
                    with open(os.path.join(d, "x86.err"), "wb") as f:
                        f.write(x_err)
        if problems:
            failures += 1
            kinds[problems[0].split(":")[0]] = kinds.get(problems[0].split(":")[0], 0) + 1
            if failures <= 20:
                print(os.path.relpath(d, ROOT) + ": " + problems[0][:300])
        else:
            shutil.rmtree(d, ignore_errors=True)
    for kind, count in sorted(kinds.items(), key=lambda kv: -kv[1]):
        print(f"  {count:5}  {kind}")
    print(f"{args.cases} cases, seed {args.seed}: {failures} failing (kept under target/arm64_fuzz/)")
    sys.exit(min(failures, 100))


if __name__ == "__main__":
    main()
