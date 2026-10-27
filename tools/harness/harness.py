#!/usr/bin/env python3
"""The A/B harness (README.md): the same tasks written by models in AIPL,
Python, and Rust, scored the same way.

    harness.py gen                              write each task's tests from its ref.py
    harness.py prompt TASK LANG                 print the prompt for one task
    harness.py prompts LANG DIR                 write every task's prompt to DIR/TASK.prompt.txt
    harness.py check TASK LANG FILE             compile and test one program (or a model's reply)
    harness.py score LANG DIR --model NAME [--round K] [--attempt A]
                                                score DIR/TASK.reply[.K].txt, record results,
                                                and write DIR/TASK.feedback.K.txt for each failure
    harness.py run LANG --model NAME --model-cmd CMD [--rounds R] [--attempts A] [--tasks T,...]
                                                ask a model through CMD (prompt on stdin, reply on
                                                stdout), with up to R rounds of feedback each
    harness.py report [RESULTS.jsonl...]        summarize recorded results
    harness.py selftest                         each ref.py passes its own tests, and the AIPL,
                                                Python, and Rust paths compile and run a program,
                                                and the workspace holds only what an agent may see
    harness.py workspace DIR [--docs guide,spec]
                                                a folder for an agent with a shell: those documents,
                                                bin/aipl and bin/aipl-run, the task texts, and an
                                                empty solutions/ (no tests, references, or examples)
    harness.py score-workspace DIR --model NAME [--attempt A]
                                                score DIR/solutions/NN_name.aipl on the hidden tests

LANG is aipl, python, or rust. Results go to results/MODEL.jsonl, one line
per program scored.
"""
import argparse
import datetime
import importlib.util
import json
import os
import random
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
TASKS = os.path.join(HERE, "tasks")
RESULTS = os.path.join(HERE, "results")
GUIDE = os.path.join(ROOT, "PROMPT_GUIDE_FOR_AIS.md")
LANGS = ("aipl", "python", "rust")
TIMEOUT = 10  # seconds per test run
COMPILE_TIMEOUT = 120
SHOWN = 2000  # characters of an input, output, or error shown in feedback


def tool(name):
    """The aipl or aipl-run binary: $AIPL_BIN_DIR, the repository's
    target/release, or PATH."""
    for d in (os.environ.get("AIPL_BIN_DIR"), os.path.join(ROOT, "target", "release")):
        if d and os.path.exists(os.path.join(d, name)):
            return os.path.join(d, name)
    found = shutil.which(name)
    if not found:
        sys.exit(f"harness: no {name}: build it (cargo build --release) or set AIPL_BIN_DIR")
    return found


def task_names():
    return sorted(d for d in os.listdir(TASKS) if os.path.isdir(os.path.join(TASKS, d)))


def load_ref(task):
    spec = importlib.util.spec_from_file_location(f"ref_{task}", os.path.join(TASKS, task, "ref.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def tests(task):
    """[(name, input, expected)] in order."""
    d = os.path.join(TASKS, task, "tests")
    names = sorted(f[:-3] for f in os.listdir(d) if f.endswith(".in"))
    return [(n, open(os.path.join(d, n + ".in"), encoding="latin-1").read(),
             open(os.path.join(d, n + ".out"), encoding="latin-1").read()) for n in names]


# ----- prompts -----

LANG_ASK = {
    "aipl": "Write the program in AIPL, the language documented above. It reads standard input and writes standard output.",
    "python": "Write the program in Python 3, using only the standard library. It reads standard input and writes standard output.",
    "rust": "Write the program in Rust (edition 2021), using only the standard library, as a single file with `fn main`. It reads standard input and writes standard output.",
}
FENCE = {"aipl": "aipl", "python": "python", "rust": "rust"}


def prompt(task, lang):
    spec = open(os.path.join(TASKS, task, "task.md")).read()
    parts = []
    if lang == "aipl":
        parts.append("The documentation of the AIPL programming language follows, then a task.\n\n" + open(GUIDE).read())
        parts.append("---\n")
    parts.append(spec)
    parts.append(f"{LANG_ASK[lang]} Reply with the complete program in one ```{FENCE[lang]} code block.\n")
    return "\n".join(parts)


def feedback(result):
    """The follow-up message for a failed program, the same for every language."""
    cut = lambda s: s if len(s) <= SHOWN else s[:SHOWN] + "\n[... cut]"
    if not result["compiled"]:
        return f"Your program did not compile:\n\n{cut(result['compile_error'])}\n\nReply with the whole corrected program in one code block.\n"
    f = result["failure"]
    what = {"wrong_answer": "gave a wrong answer", "runtime_error": "stopped with an error", "timeout": f"did not finish in {TIMEOUT} seconds"}[f["kind"]]
    msg = f"Your program compiled, but on one test it {what}.\n\nInput:\n{cut(f['input'])}\n"
    if f["kind"] != "timeout":
        msg += f"Expected output:\n{cut(f['expected'])}\nYour output:\n{cut(f['got'])}\n"
    if f.get("stderr"):
        msg += f"Standard error:\n{cut(f['stderr'])}\n"
    return msg + "\nReply with the whole corrected program in one code block.\n"


def extract(reply):
    """The program in a reply: its first fenced code block, or all of it."""
    m = re.search(r"```[A-Za-z0-9_+-]*[^\n]*\n(.*?)```", reply, re.S)
    return m.group(1) if m else reply


# ----- compiling and testing -----

def build(lang, code, d):
    """(compiled, error, command to run)."""
    if lang == "python":
        path = os.path.join(d, "prog.py")
        open(path, "w").write(code)
        r = subprocess.run([sys.executable, "-m", "py_compile", path], capture_output=True, text=True)
        return r.returncode == 0, r.stderr, [sys.executable, path]
    if lang == "rust":
        path = os.path.join(d, "prog.rs")
        open(path, "w").write(code)
        exe = os.path.join(d, "prog")
        try:
            r = subprocess.run(["rustc", "--edition", "2021", "-O", "-o", exe, path], capture_output=True, text=True, timeout=COMPILE_TIMEOUT)
        except subprocess.TimeoutExpired:
            return False, "rustc took too long", None
        return r.returncode == 0, r.stderr, [exe]
    path = os.path.join(d, "prog.aipl")
    open(path, "w").write(code)
    wasm = os.path.join(d, "prog.wasm")
    try:
        r = subprocess.run([tool("aipl"), "compile", path, "-o", wasm], capture_output=True, text=True, timeout=COMPILE_TIMEOUT)
    except subprocess.TimeoutExpired:
        return False, "aipl compile took too long", None
    return r.returncode == 0, (r.stderr + r.stdout).replace(d + os.sep, ""), [tool("aipl-run"), wasm]


def check_code(task, lang, code):
    """Compiles code and runs it on every test; stops at the first failure."""
    with tempfile.TemporaryDirectory(prefix="harness_") as d:
        ok, err, cmd = build(lang, code, d)
        result = {"compiled": ok, "compile_error": err.strip() if not ok else "", "tests": 0, "passed": 0, "failure": None}
        if not ok:
            return result
        ts = tests(task)
        result["tests"] = len(ts)
        for name, inp, want in ts:
            try:
                r = subprocess.run(cmd, input=inp.encode("latin-1"), capture_output=True, timeout=TIMEOUT, cwd=d)
            except subprocess.TimeoutExpired:
                result["failure"] = {"kind": "timeout", "test": name, "input": inp}
                break
            got = r.stdout.decode("latin-1")
            if r.returncode != 0:
                result["failure"] = {"kind": "runtime_error", "test": name, "input": inp, "expected": want, "got": got,
                                     "stderr": r.stderr.decode("utf-8", "replace").replace(d + os.sep, "").strip(), "status": r.returncode}
                break
            if got != want:
                result["failure"] = {"kind": "wrong_answer", "test": name, "input": inp, "expected": want, "got": got}
                break
            result["passed"] += 1
        return result


def record(model, task, lang, attempt, rnd, code, result, prompt_chars, reply_chars):
    os.makedirs(RESULTS, exist_ok=True)
    line = {
        "time": datetime.datetime.now().isoformat(timespec="seconds"), "model": model, "task": task, "lang": lang,
        "attempt": attempt, "round": rnd, "compiled": result["compiled"], "passed_all": result["compiled"] and result["failure"] is None,
        "tests": result["tests"], "tests_passed": result["passed"],
        "failure": "compile_error" if not result["compiled"] else (result["failure"]["kind"] if result["failure"] else None),
        "program_bytes": len(code.encode()), "program_lines": len([l for l in code.split("\n") if l.strip()]),
        "prompt_chars": prompt_chars, "reply_chars": reply_chars, "aipl_version": aipl_version() if lang == "aipl" else None,
    }
    with open(os.path.join(RESULTS, re.sub(r"[^A-Za-z0-9_.-]", "_", model) + ".jsonl"), "a") as f:
        f.write(json.dumps(line) + "\n")
    return line


_VERSION = None


def aipl_version():
    global _VERSION
    if _VERSION is None:
        _VERSION = subprocess.run([tool("aipl"), "--version"], capture_output=True, text=True).stdout.strip()
    return _VERSION


def verdict(result):
    if not result["compiled"]:
        return "does not compile"
    if result["failure"]:
        return f"{result['failure']['kind']} on test {result['failure']['test']} ({result['passed']}/{result['tests']} passed)"
    return f"passed {result['tests']}/{result['tests']}"


# ----- commands -----

def cmd_gen(args):
    for t in task_names():
        m = load_ref(t)
        d = os.path.join(TASKS, t, "tests")
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
        for k, inp in enumerate(m.cases(random.Random(f"{t}:1"))):
            out = m.solve(inp)
            open(os.path.join(d, f"{k:02d}.in"), "w", encoding="latin-1", newline="").write(inp)
            open(os.path.join(d, f"{k:02d}.out"), "w", encoding="latin-1", newline="").write(out)
        print(f"{t}: {k + 1} tests")


def cmd_prompt(args):
    sys.stdout.write(prompt(args.task, args.lang))


def cmd_prompts(args):
    os.makedirs(args.dir, exist_ok=True)
    for t in task_names():
        open(os.path.join(args.dir, f"{t}.prompt.txt"), "w").write(prompt(t, args.lang))
    print(f"{len(task_names())} prompts in {args.dir}: paste each into a fresh chat, and save the reply as TASK.reply.txt "
          f"(a later round's as TASK.reply.2.txt, ...), then: harness.py score {args.lang} {args.dir} --model NAME")


def cmd_check(args):
    code = extract(open(args.file).read())
    result = check_code(args.task, args.lang, code)
    print(verdict(result))
    if not result["passed"] == result["tests"] or not result["compiled"]:
        print(feedback(result))
    sys.exit(0 if result["compiled"] and not result["failure"] else 1)


def cmd_score(args):
    suffix = "" if args.round == 1 else f".{args.round}"
    scored = failed = 0
    for t in task_names():
        path = os.path.join(args.dir, f"{t}.reply{suffix}.txt")
        if not os.path.exists(path):
            continue
        reply = open(path).read()
        code = extract(reply)
        result = check_code(t, args.lang, code)
        record(args.model, t, args.lang, args.attempt, args.round, code, result, len(prompt(t, args.lang)), len(reply))
        print(f"{t}: {verdict(result)}")
        if not result["compiled"] or result["failure"]:
            open(os.path.join(args.dir, f"{t}.feedback.{args.round}.txt"), "w").write(feedback(result))
        scored += 1
        failed += not result["compiled"] or result["failure"] is not None
    print(f"{scored} replies scored, {failed} failing.")
    if failed:
        print(f"Each failure's follow-up is in TASK.feedback.{args.round}.txt: paste it into the same chat, save the answer as "
              f"TASK.reply.{args.round + 1}.txt, and score it with --round {args.round + 1}.")


def cmd_run(args):
    names = args.tasks.split(",") if args.tasks else task_names()
    for t in names:
        for attempt in range(1, args.attempts + 1):
            first = prompt(t, args.lang)
            convo = first
            for rnd in range(1, args.rounds + 1):
                r = subprocess.run(args.model_cmd, shell=True, input=convo, capture_output=True, text=True)
                reply = r.stdout
                code = extract(reply)
                result = check_code(t, args.lang, code)
                record(args.model, t, args.lang, attempt, rnd, code, result, len(convo), len(reply))
                print(f"{t} attempt {attempt} round {rnd}: {verdict(result)}", flush=True)
                if result["compiled"] and not result["failure"]:
                    break
                # the whole conversation each time: the command need not remember
                convo += f"\n\n--- Your reply ---\n{reply}\n\n--- Feedback ---\n{feedback(result)}"


def cmd_report(args):
    files = args.files
    if not files and os.path.isdir(RESULTS):
        files = [os.path.join(RESULTS, f) for f in sorted(os.listdir(RESULTS)) if f.endswith(".jsonl")]
    rows = [json.loads(l) for f in files or [] for l in open(f) if l.strip()]
    if not rows:
        print("no results")
        return
    groups = {}
    for r in rows:
        groups.setdefault((r["model"], r["lang"]), []).append(r)
    print("| model | language | runs | compiles first try | passes first try | passes within its rounds | mean rounds to pass | failures, by kind | mean lines (passing) |")
    print("|---|---|---|---|---|---|---|---|---|")
    for (model, lang), rs in sorted(groups.items()):
        # one run is one task's attempt, over its rounds
        runs = {}
        for r in rs:
            runs.setdefault((r["task"], r["attempt"]), []).append(r)
        n = len(runs)
        firsts = [min(v, key=lambda x: x["round"]) for v in runs.values()]
        won = [min(x["round"] for x in v if x["passed_all"]) for v in runs.values() if any(x["passed_all"] for x in v)]
        kinds = {}
        for r in rs:
            if r["failure"]:
                kinds[r["failure"]] = kinds.get(r["failure"], 0) + 1
        lines = [x["program_lines"] for x in rs if x["passed_all"]]
        pct = lambda k: f"{100 * k / n:.0f}% ({k}/{n})"
        cells = [model, lang, str(n), pct(sum(f["compiled"] for f in firsts)), pct(sum(f["passed_all"] for f in firsts)), pct(len(won)),
                 f"{sum(won) / len(won):.2f}" if won else "-", ", ".join(f"{k} {v}" for k, v in sorted(kinds.items())) or "-",
                 f"{sum(lines) / len(lines):.0f}" if lines else "-"]
        print("| " + " | ".join(cells) + " |")


SELFTEST_PROGRAMS = {
    # a tiny echo of standard input in each language: the paths work, not the tasks
    "aipl": "(module main\n  (import io)\n  (import str)\n  (fn main [] -> i32\n    (match_result (call io.read_stdin)\n      (ok b (call io.println (call str.from_bytes b)) 0)\n      (err e 1))))\n",
    "python": "import sys\nsys.stdout.write(sys.stdin.read() + '\\n')\n",
    "rust": "use std::io::Read;\nfn main() { let mut s = String::new(); std::io::stdin().read_to_string(&mut s).unwrap(); println!(\"{}\", s); }\n",
}


# ----- the agent workspace -----

WORKSPACE_DOCS = {"guide": "PROMPT_GUIDE_FOR_AIS.md", "spec": "AIPL_SPEC.md"}

WORKSPACE_README = """# AIPL workspace

You are to write programs in AIPL, a programming language you may not have
seen. Everything you need is in this folder:

{docs}- `bin/aipl` and `bin/aipl-run`: the compiler and the runner.
- `tasks/`: one `NN_name.md` per task. Each describes a program that reads
  standard input and writes standard output, with one example.
- `solutions/`: where your programs go.

For every task, write `solutions/NN_name.aipl`: one file, one module, that
solves the task. Check a program the way it will be run:

    bin/aipl verify solutions/NN_name.aipl                       # type-check it
    bin/aipl compile solutions/NN_name.aipl -o /tmp/p.wasm       # build it
    bin/aipl-run /tmp/p.wasm < input.txt                         # run it

`bin/aipl doc --find WORD` searches the standard library, and
`bin/aipl doc MODULE` lists a module's functions.

Your programs are tested afterwards on inputs you cannot see: edge cases
and large inputs, each with 10 seconds. The output must match exactly.
Use nothing outside this folder: no other files, no network.
"""


def cmd_workspace(args):
    """A folder for an agent with a shell: the documents it is allowed, the binaries, the task
    texts, and nothing else (no reference solutions, no tests, no examples, no git history)."""
    d = args.dir
    if os.path.exists(d) and os.listdir(d):
        sys.exit(f"harness: {d} is not empty")
    names = [n for n in args.docs.split(",") if n]
    for n in names:
        if n not in WORKSPACE_DOCS:
            sys.exit(f"harness: no document '{n}' (choose from {', '.join(WORKSPACE_DOCS)})")
    for sub in ("bin", "tasks", "solutions") + (("docs",) if names else ()):
        os.makedirs(os.path.join(d, sub), exist_ok=True)
    for n in names:
        shutil.copy(os.path.join(ROOT, WORKSPACE_DOCS[n]), os.path.join(d, "docs", WORKSPACE_DOCS[n]))
    for b in ("aipl", "aipl-run"):
        shutil.copy(tool(b), os.path.join(d, "bin", b))
        os.chmod(os.path.join(d, "bin", b), 0o755)
    for t in task_names():
        shutil.copy(os.path.join(TASKS, t, "task.md"), os.path.join(d, "tasks", t + ".md"))
    docs = "".join(f"- `docs/{WORKSPACE_DOCS[n]}`: {'read this first: a guide to writing AIPL' if n == 'guide' else 'the full reference'}.\n" for n in names)
    open(os.path.join(d, "README.md"), "w").write(WORKSPACE_README.format(docs=docs))
    open(os.path.join(d, "VERSION"), "w").write(aipl_version() + "\n")
    print(f"workspace in {d}: {len(task_names())} tasks, documents: {', '.join(names) or 'none'}. Point the agent at it, and nothing else; "
          f"afterwards: harness.py score-workspace {d} --model NAME")


def workspace_files(d):
    return sorted(os.path.relpath(os.path.join(r, f), d) for r, _, fs in os.walk(d) for f in fs)


def cmd_score_workspace(args):
    """Scores DIR/solutions/NN_name.aipl on the hidden tests; a task without a solution counts as failed."""
    docs = sum(len(open(os.path.join(args.dir, "docs", f)).read()) for f in os.listdir(os.path.join(args.dir, "docs"))) if os.path.isdir(os.path.join(args.dir, "docs")) else 0
    scored = failed = 0
    for t in task_names():
        path = os.path.join(args.dir, "solutions", t + ".aipl")
        code = open(path).read() if os.path.exists(path) else ""
        if os.path.exists(path):
            result = check_code(t, "aipl", code)
        else:
            result = {"compiled": False, "compile_error": "no solution file", "tests": len(tests(t)), "passed": 0, "failure": None}
        record(args.model, t, "aipl", args.attempt, 1, code, result, docs + len(open(os.path.join(TASKS, t, "task.md")).read()), len(code))
        print(f"{t}: {verdict(result)}")
        scored += 1
        failed += not result["compiled"] or result["failure"] is not None
    print(f"{scored} tasks scored, {failed} failing.")


def workspace_selftest():
    """The workspace holds what an agent may see and nothing else, and its compiler works from it."""
    with tempfile.TemporaryDirectory(prefix="harness_ws_") as tmp:
        d = os.path.join(tmp, "ws")
        cmd_workspace(argparse.Namespace(dir=d, docs=",".join(WORKSPACE_DOCS)))
        want = sorted(["README.md", "VERSION", "bin/aipl", "bin/aipl-run"] + [f"docs/{f}" for f in WORKSPACE_DOCS.values()]
                      + [f"tasks/{t}.md" for t in task_names()])
        got = workspace_files(d)
        if got != want:
            return f"the workspace holds {sorted(set(got) ^ set(want))} beyond or short of what it should"
        if os.listdir(os.path.join(d, "solutions")):
            return "the solutions folder starts out empty"
        src = os.path.join(d, "solutions", "t.aipl")
        open(src, "w").write(SELFTEST_PROGRAMS["aipl"])
        wasm = os.path.join(tmp, "t.wasm")
        r = subprocess.run([os.path.join(d, "bin", "aipl"), "compile", src, "-o", wasm], capture_output=True, text=True, cwd=d)
        if r.returncode:
            return "the workspace's compiler does not compile: " + r.stderr.strip()
        out = subprocess.run([os.path.join(d, "bin", "aipl-run"), wasm], input=b"hi", capture_output=True, cwd=d).stdout
        if out != b"hi\n":
            return f"the workspace's runner printed {out!r}"
    return None


def cmd_selftest(args):
    bad = 0
    for t in task_names():
        code = open(os.path.join(TASKS, t, "ref.py")).read() + "\nimport sys\nsys.stdout.write(solve(sys.stdin.read()))\n"
        r = check_code(t, "python", code)
        if not (r["compiled"] and r["failure"] is None):
            bad += 1
            print(f"{t}: the reference fails its own tests: {verdict(r)}")
    for lang, code in SELFTEST_PROGRAMS.items():
        with tempfile.TemporaryDirectory(prefix="harness_") as d:
            ok, err, cmd = build(lang, code, d)
            out = subprocess.run(cmd, input=b"hi", capture_output=True, timeout=TIMEOUT).stdout if ok else b""
            if out != b"hi\n":
                bad += 1
                print(f"{lang}: the toolchain path does not work: {err.strip() or out!r}")
    why = workspace_selftest()
    if why:
        bad += 1
        print(f"workspace: {why}")
    print(f"selftest: {len(task_names())} references and {len(SELFTEST_PROGRAMS)} toolchains, {bad} failing")
    sys.exit(1 if bad else 0)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("gen")
    s = sub.add_parser("prompt"); s.add_argument("task"); s.add_argument("lang", choices=LANGS)
    s = sub.add_parser("prompts"); s.add_argument("lang", choices=LANGS); s.add_argument("dir")
    s = sub.add_parser("check"); s.add_argument("task"); s.add_argument("lang", choices=LANGS); s.add_argument("file")
    s = sub.add_parser("score"); s.add_argument("lang", choices=LANGS); s.add_argument("dir"); s.add_argument("--model", required=True)
    s.add_argument("--round", type=int, default=1); s.add_argument("--attempt", type=int, default=1)
    s = sub.add_parser("run"); s.add_argument("lang", choices=LANGS); s.add_argument("--model", required=True); s.add_argument("--model-cmd", required=True)
    s.add_argument("--rounds", type=int, default=3); s.add_argument("--attempts", type=int, default=1); s.add_argument("--tasks")
    s = sub.add_parser("report"); s.add_argument("files", nargs="*")
    sub.add_parser("selftest")
    s = sub.add_parser("workspace"); s.add_argument("dir"); s.add_argument("--docs", default=",".join(WORKSPACE_DOCS))
    s = sub.add_parser("score-workspace"); s.add_argument("dir"); s.add_argument("--model", required=True); s.add_argument("--attempt", type=int, default=1)
    args = p.parse_args()
    {"gen": cmd_gen, "prompt": cmd_prompt, "prompts": cmd_prompts, "check": cmd_check, "score": cmd_score,
     "run": cmd_run, "report": cmd_report, "selftest": cmd_selftest, "workspace": cmd_workspace,
     "score-workspace": cmd_score_workspace}[args.cmd](args)


if __name__ == "__main__":
    main()
