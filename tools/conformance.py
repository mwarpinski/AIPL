#!/usr/bin/env python3
"""Runs the conformance suite, tests/conformance/, against an AIPL implementation (AIPL_SPEC.md 6.4).

    python3 tools/conformance.py [--aipl PATH] [--backend NAME]... [--section ID] [-k TEXT] [--list] [-v]
    python3 tools/conformance.py --check-cases        (the suite's own consistency; needs no implementation)

A case is one `.aipl` file under tests/conformance/ whose leading comments carry directives:

    ;; @spec 5.4          the sections of AIPL_SPEC.md it tests (space separated; at least one)
    ;; @kind run          run | trap | reject
    ;; @args a b          the program's arguments after its name
    ;; @stdin text        what is on standard input (\\n and \\t are escapes)
    ;; @out a line        a line of standard output, in order, each ended by a newline (repeat for more lines);
                          `@out-raw` ends none. A run case with no @out must write nothing to standard output.
    ;; @err text          standard error contains this (any case)
    ;; @syntax          (a reject only) the error is one of syntax: the program does not match the grammar of AIPL_SPEC.md 2
                          (tools/grammar_check.py checks that it does not, and that every other case does)
    ;; @exit N            the exit status (run: default 0; trap: any value but 0 unless given)
    ;; @trap a | b        a trap: a failure whose message contains one of the alternatives (any case)
    ;; @reject L:C text   a reject: the program is refused at line L, column C, with a message containing `text`
                          (or any `|` alternative of it). `@reject - text` is for the errors that have no position
                          (an import that cannot be found): refused, with that message.

A file without `@kind` is not a case: it is a module that a case imports (tests/conformance/imports/).

How a case is run is the implementation's business. The built-in adapters drive the `aipl` of this repository:

    run     `aipl run FILE ARGS...`              (the toolchain's way: compile, then run)
    wasm    `aipl compile FILE`, then `aipl run OUT.wasm ARGS...`
    native  `aipl compile --exe FILE`, then OUT ARGS...
    check   `aipl verify FILE`                   (reject cases; always used for them)

Another implementation names its own commands, with {file} {args} {out} {dir} placeholders:

    --run-cmd 'mylang run {file} {args}'    --check-cmd 'mylang check {file}'

Reads the standard library from AIPL_STD (default: aipl_src/std in this repository). Exit status 1 if any case fails.
"""
import argparse
import concurrent.futures
import os
import re
import shlex
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SUITE = os.path.join(ROOT, "tests", "conformance")
DIRECTIVE = re.compile(r"^;;\s*@([a-z-]+)(?:\s+(.*))?$")
KINDS = ("run", "trap", "reject")


class Case:
    def __init__(self, path):
        self.path = path
        self.name = os.path.relpath(path, SUITE)[:-5]
        self.d = {"spec": [], "out": [], "kind": None}
        self.errors = []
        for line in open(path, encoding="utf-8").read().split("\n"):
            m = DIRECTIVE.match(line.rstrip())
            if not m:
                continue
            key, val = m.group(1), (m.group(2) or "")
            if key in ("out", "out-raw"):
                self.d["out"].append((key, val))
            elif key == "spec":
                self.d["spec"] += val.split()
            elif key == "syntax":
                self.d["syntax"] = True
            elif key in ("kind", "args", "stdin", "err", "exit", "trap", "reject"):
                self.d[key] = val
            else:
                self.errors.append(f"unknown directive @{key}")
        if self.is_case:
            if self.d["kind"] not in KINDS:
                self.errors.append(f"@kind is {self.d['kind']!r}: run, trap, or reject")
            if not self.d["spec"]:
                self.errors.append("no @spec")
            if self.d["kind"] == "trap" and "trap" not in self.d:
                self.errors.append("a trap case needs @trap")
            if self.d["kind"] == "reject" and not re.match(r"^(\d+:\d+|-) \S", self.d.get("reject", "")):
                self.errors.append("a reject case needs @reject L:C text (or @reject - text)")
            if self.d.get("exit") is not None and not re.fullmatch(r"\d+", self.d["exit"]):
                self.errors.append("@exit is a number")

    @property
    def is_case(self):
        return self.d["kind"] is not None

    @property
    def kind(self):
        return self.d["kind"]

    def expected_stdout(self):
        out = ""
        for key, val in self.d["out"]:
            out += val + ("" if key == "out-raw" else "\n")
        return out


def discover():
    cases = []
    for d, dirs, files in os.walk(SUITE):
        dirs.sort()
        for f in sorted(files):
            if f.endswith(".aipl"):
                cases.append(Case(os.path.join(d, f)))
    return cases


def spec_sections():
    """The section ids AIPL_SPEC.md defines (4, 4.A, 5.4, ...), to check every @spec names one."""
    ids, top = set(), None
    for line in open(os.path.join(ROOT, "AIPL_SPEC.md"), encoding="utf-8"):
        m = re.match(r"^## (\d+)\.", line)
        if m:
            top = m.group(1)
            ids.add(top)
        m = re.match(r"^### (\d+\.\d+) ", line)
        if m:
            ids.add(m.group(1))
        m = re.match(r"^### ([A-Z])\. ", line)
        if m and top:
            ids.add(f"{top}.{m.group(1)}")
    return ids


def check_cases(cases):
    findings = []
    valid = spec_sections()
    seen = set()
    for c in cases:
        for e in c.errors:
            findings.append(f"{c.name}: {e}")
        if c.is_case:
            for s in c.d["spec"]:
                if s not in valid:
                    findings.append(f"{c.name}: @spec {s}: AIPL_SPEC.md has no such section")
                seen.add(s)
            if c.d.get("syntax") and c.kind != "reject":
                findings.append(f"{c.name}: @syntax is for a reject case")
            if c.kind != "run" and c.d["out"] and c.kind == "reject":
                findings.append(f"{c.name}: a reject case writes nothing")
            if not re.search(r"\(module\s+" + re.escape(os.path.basename(c.name)) + r"\b", open(c.path, encoding="utf-8").read()):
                findings.append(f"{c.name}: the module is not named after the file ({os.path.basename(c.name)})")
    return findings, seen


# ------------------------------------------------------------------ adapters

def env_for(args):
    env = dict(os.environ)
    std = env.get("AIPL_STD") or os.path.join(ROOT, "aipl_src", "std")
    if os.path.isdir(std):
        env["AIPL_STD"] = std
    return env


def run_cmd(cmd, stdin, cwd, env, timeout=120):
    try:
        p = subprocess.run(cmd, input=stdin.encode(), capture_output=True, cwd=cwd, env=env, timeout=timeout)
    except subprocess.TimeoutExpired:
        return 124, "", "timed out"
    return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")


def split_args(c):
    return shlex.split(c.d.get("args", ""))


def build_runner(args, backend):
    """A function (case, workdir) -> (status, stdout, stderr)."""
    aipl = args.aipl
    env = env_for(args)

    def templated(tpl, c, work, out=""):
        words = shlex.split(tpl)
        cmd = []
        for w in words:
            if w == "{args}":
                cmd += split_args(c)
            else:
                cmd.append(w.replace("{file}", c.path).replace("{out}", out).replace("{dir}", work))
        return cmd

    if args.run_cmd:
        return lambda c, work: run_cmd(templated(args.run_cmd, c, work, os.path.join(work, "prog")), unescape(c.d.get("stdin", "")), work, env)
    if backend == "run":
        return lambda c, work: run_cmd([aipl, "run", c.path] + split_args(c), unescape(c.d.get("stdin", "")), work, env)
    if backend == "wasm":
        def go(c, work):
            wasm = os.path.join(work, "prog.wasm")
            st, o, e = run_cmd([aipl, "compile", c.path, "-o", wasm], "", work, env)
            if st != 0:
                return st, o, e
            return run_cmd([aipl, "run", wasm] + split_args(c), unescape(c.d.get("stdin", "")), work, env)
        return go
    if backend == "native":
        def go(c, work):
            exe = os.path.join(work, "prog")
            st, o, e = run_cmd([aipl, "compile", "--exe", c.path, "-o", exe], "", work, env)
            if st != 0:
                return st, o, e
            return run_cmd([exe] + split_args(c), unescape(c.d.get("stdin", "")), work, env)
        return go
    raise SystemExit(f"conformance: no backend {backend}")


def build_checker(args):
    env = env_for(args)
    if args.check_cmd:
        def go(c, work):
            cmd = [w.replace("{file}", c.path).replace("{dir}", work) for w in shlex.split(args.check_cmd)]
            return run_cmd(cmd, "", work, env)
        return go
    return lambda c, work: run_cmd([args.aipl, "verify", c.path], "", work, env)


def unescape(s):
    return s.replace("\\n", "\n").replace("\\t", "\t")


# ------------------------------------------------------------------ judging

def judge_run(c, status, out, err):
    problems = []
    want = int(c.d["exit"]) if c.d.get("exit") is not None else 0
    if status != want:
        problems.append(f"exit status {status}, wanted {want}")
    if out != c.expected_stdout():
        problems.append(f"standard output {out!r}, wanted {c.expected_stdout()!r}")
    if c.d.get("err") and c.d["err"].lower() not in err.lower():
        problems.append(f"standard error lacks {c.d['err']!r}: {err.strip()[:200]!r}")
    return problems


def judge_trap(c, status, out, err):
    problems = []
    if status == 0:
        problems.append("the program did not trap (exit status 0)")
    elif c.d.get("exit") is not None and status != int(c.d["exit"]):
        problems.append(f"exit status {status}, wanted {c.d['exit']}")
    alts = [a.strip().lower() for a in c.d["trap"].split("|")]
    if not any(a in err.lower() for a in alts):
        problems.append(f"standard error has none of {alts}: {err.strip()[:300]!r}")
    if c.d["out"] and out != c.expected_stdout():
        problems.append(f"standard output {out!r}, wanted {c.expected_stdout()!r}")
    if c.d.get("err") and c.d["err"].lower() not in err.lower():
        problems.append(f"standard error lacks {c.d['err']!r}")
    return problems


def judge_reject(c, status, out, err):
    if status == 0:
        return ["the program was accepted"]
    pos, _, text = c.d["reject"].partition(" ")
    alts = [a.strip().lower() for a in text.split("|")]
    if pos == "-":
        return [] if any(a in err.lower() for a in alts) else [f"the message has none of {alts}: {err.strip()[:200]!r}"]
    # the first line:col: in the message is the error's place
    m = re.search(r"(?<![0-9])(\d+):(\d+):\s", err)
    if not m:
        return [f"no line:col in the message: {err.strip()[:200]!r}"]
    problems = []
    if f"{m.group(1)}:{m.group(2)}" != pos:
        problems.append(f"refused at {m.group(1)}:{m.group(2)}, wanted {pos}")
    if not any(a in err.lower() for a in alts):
        problems.append(f"the message has none of {alts}: {err.strip()[:200]!r}")
    return problems


def run_case(c, backend, runner, checker):
    with tempfile.TemporaryDirectory(prefix="aipl-conf-") as work:
        if c.kind == "reject":
            st, out, err = checker(c, work)
            return judge_reject(c, st, out, err)
        st, out, err = runner(c, work)
        return (judge_run if c.kind == "run" else judge_trap)(c, st, out, err)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--aipl", default=os.environ.get("AIPL") or os.path.join(ROOT, "target", "release", "aipl"))
    ap.add_argument("--backend", action="append", choices=["run", "wasm", "native"])
    ap.add_argument("--run-cmd")
    ap.add_argument("--check-cmd")
    ap.add_argument("--section")
    ap.add_argument("-k", dest="match")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--check-cases", action="store_true")
    ap.add_argument("-j", "--jobs", type=int, default=4)
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    args.aipl = os.path.abspath(args.aipl)    # the cases run in a scratch directory of their own

    all_cases = discover()
    findings, seen = check_cases(all_cases)
    cases = [c for c in all_cases if c.is_case]
    if args.check_cases or args.list:
        for f in findings:
            print(f)
        if args.list:
            for c in cases:
                print(f"{c.kind:6} {' '.join(c.d['spec']):10} {c.name}")
        print(f"{len(cases)} cases, {len(seen)} sections tested, {len(findings)} findings")
        return 1 if findings else 0
    if findings:
        for f in findings:
            print(f)
        print(f"{len(findings)} findings in the suite")
        return 1
    if args.section:
        cases = [c for c in cases if any(s == args.section or s.startswith(args.section + ".") for s in c.d["spec"])]
    if args.match:
        cases = [c for c in cases if args.match in c.name]
    backends = args.backend or ["run"]
    if args.run_cmd:
        backends = ["custom"]
    if not args.run_cmd and not os.path.exists(args.aipl):
        sys.exit(f"conformance: no aipl at {args.aipl} (--aipl PATH, or AIPL=PATH)")
    checker = build_checker(args)
    failed = 0
    for backend in backends:
        runner = build_runner(args, backend)
        todo = [c for c in cases if not (backend != backends[0] and c.kind == "reject")]
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            results = list(pool.map(lambda c: run_case(c, backend, runner, checker), todo))
        bad = 0
        for c, problems in zip(todo, results):
            if problems:
                bad += 1
                print(f"FAIL [{backend}] {c.name} (AIPL_SPEC.md {' '.join(c.d['spec'])})")
                for p in problems:
                    print(f"    {p}")
            elif args.verbose:
                print(f"ok   [{backend}] {c.name}")
        print(f"{backend}: {len(todo) - bad} of {len(todo)} cases pass")
        failed += bad
    print(f"{failed} failing")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
