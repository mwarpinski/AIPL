#!/usr/bin/env python3
"""Checks the documentation against the repository (docs/LAUNCH_CHECKLIST.md,
section 8: "the documentation check is rerun").

    python3 tools/docs_check.py [--all]

1. Every file the documentation names exists: a `path/like/this.ext` in
   backticks, a Markdown link, and std/NAME (std/NAME.aipl under aipl_src/).
   A `:LINE` or `:A-B` after a path is dropped. The archives
   (docs/history, docs/notes) and the dated records (docs/design/AGENT_FRICTION.md,
   docs/NOTES_REVIEW.md, docs/HANDOFF.md's history) are skipped unless --all.
2. Words the language no longer has are not used as current (STALE below), in
   the documents a reader meets first.
3. The numbers that can be counted are the numbers written: the test files
   and the standard library's modules.
4. bootstrap/README.md's seed hashes (x86-64 and AArch64) are the checked-in seeds'.
5. Every `module.name` the documents write is exported by that module (asked
   of `aipl doc`), every `--flag` in the spec's command table is one a command
   takes, every form is in the spec, every "section N.X" and "AIPL_SPEC.md
   N.X" names a heading, and the library's own comments use no gone form.

Prints one line per finding and exits with their count (capped at 100).
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# archives and dated records: what they name may be gone, as the record says
ARCHIVES = ("docs/history/", "docs/notes/", "docs/design/AGENT_FRICTION.md", "docs/NOTES_REVIEW.md", "docs/APPLE_SILICON")
# the documents a reader meets first
FRONT = ("README.md", "AIPL_SPEC.md", "PROMPT_GUIDE_FOR_AIS.md", "docs/FAQ.md", "docs/TOUR.md", "docs/USING_AIPL.md", "ROADMAP.md", "DEVELOPING.md",
         "bootstrap/README.md", "tools/harness/README.md")

# (regex, what is wrong), checked in FRONT only, outside a line that says it is history
STALE = [
    (r"\bio\.(print_int|println_int|print_i64|println_i64|print_f64|println_f64)\b", "io's labelled print helpers are gone: use the format forms"),
    (r"one type per function|keeps one type per name", "a name's scope is its block or arm now"),
    (r"\(loop [a-z_]+ ", "loop is retired: for"),
    (r"\bThere is no array literal\b", "arr.of is the array literal"),
    (r"len -1 (if|when|for) (unset|missing)|returns? len -1 for a missing", "os.env and os.arg give an Option now"),
]
# looks like module.name, is not: a placeholder, a typo shown on purpose, a generated name
NOT_NAMES = {("io", "something"), ("alloc", "something"), ("show", "debug"), ("text", "splt"), ("vec", "psh"),
             ("http", "fetch")}  # planned: NET_PLAN.md step 5
HISTORY_HINT = re.compile(r"\b(was|were|used to|until|since|before|formerly|retired|gone|removed|renamed|replaced|old|once|historical|2026-)\b", re.I)

PATH = re.compile(r"`([A-Za-z0-9_./~$*-]+(?:/[A-Za-z0-9_.*-]+)+)(?::[0-9][0-9,-]*)?`")
LINK = re.compile(r"\]\(([^)#\s]+)(?:#[^)]*)?\)")
KNOWN_ROOTS = ("src/", "aipl_src/", "tests/", "tools/", "docs/", "examples/", "benchmarks/", "bootstrap/", ".github/", "std/", "attic/")


def tracked():
    return subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True).stdout.split("\n")


def exists(files, dirs, path, doc):
    path = path.rstrip("/")
    cands = [path, os.path.normpath(os.path.join(os.path.dirname(doc), path))]
    if path.startswith("std/"):
        mod = path.split(".")[0]  # std/float.from_decimal is std/float's from_decimal
        cands += ["aipl_src/" + path, "aipl_src/" + path + ".aipl", "aipl_src/" + mod + ".aipl"]
    for c in cands:
        if "*" in c:
            rx = re.compile("^" + re.escape(c).replace(r"\*", "[^/]*") + "$")
            if any(rx.match(f) for f in files):
                return True
        elif c in files or c in dirs:
            return True
    return False


def run(*cmd):
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True).stdout


def aipl():
    exe = os.path.join(ROOT, "target", "release", "aipl")
    if not os.path.exists(exe):
        sys.exit("docs_check: build first (cargo build --release): it asks aipl doc what the library exports")
    return exe


def exports(files):
    """{module: {public names}} from `aipl doc`, for every standard module."""
    out = {}
    for f in sorted(files):
        m = re.match(r"aipl_src/std/([a-z0-9_]+)\.aipl$", f)
        if not m:
            continue
        names = set()
        for line in run(aipl(), "doc", m.group(1)).split("\n"):
            d = re.match(r"^(?:fn|struct|union|enum|const|interface)\s+\(?%s\.([A-Za-z0-9_]+)" % m.group(1), line)
            if d:
                names.add(d.group(1))
        out[m.group(1)] = names
    return out


def check_names(files, docs, show_all):
    """Every module.name the documents write exists in that module (or is a
    built-in operation, like fs.write), and every `name [params]` in the
    spec's standard-library table is in its row's module."""
    mods = exports(files)
    heads = set(re.findall(r'"([a-z0-9_.]+)"', open(os.path.join(ROOT, "src/parser.rs"), encoding="utf-8").read().split("pub const HEAD_NAMES")[1].split("];")[0]))
    findings = []
    # words that are not module names although a module has the name (a file, a field, an English word)
    for doc in docs:
        if doc.startswith(ARCHIVES) and not show_all:
            continue
        in_fence = False
        for n, line in enumerate(open(os.path.join(ROOT, doc), encoding="utf-8"), 1):
            if line.lstrip().startswith("```"):
                in_fence = not in_fence
            # the design plans are dated records of what was planned; a name in
            # one may have changed since (its status line says what became of it)
            if in_fence or doc.startswith("docs/design/") or "python" in line.lower():
                continue
            for m in re.finditer(r"(?<![A-Za-z0-9_./\\-])([a-z][a-z0-9_]*)\.([a-z_][A-Za-z0-9_]*)(?![A-Za-z0-9_]*[./(]\w)", line):
                mod, name = m.groups()
                if name in ("aipl", "rs", "wasm", "md", "txt", "py", "sh", "json", "toml", "yml", "dumps") or (mod, name) in NOT_NAMES:
                    continue
                if mod in mods and name not in mods[mod] and f"{mod}.{name}" not in heads and not re.search(r"\b(was|were|removed|gone|retired|renamed|replaced)\b|^\s*[|-].*io\.(print|println)_", line, re.I):
                    if re.search(r"`[^`]*\b%s\.%s\b" % (mod, re.escape(name)), line) or re.search(r"\(call \(?%s\.%s\b" % (mod, re.escape(name)), line):
                        findings.append(f"{doc}:{n}: {mod}.{name} is not exported by std/{mod}")
    spec = open(os.path.join(ROOT, "AIPL_SPEC.md"), encoding="utf-8").read().split("\n")
    for n, line in enumerate(spec, 1):
        row = re.match(r"^\| `([a-z0-9_]+)` \| (.*)", line)
        if not row or row.group(1) not in mods:
            continue
        mod = row.group(1)
        for tok in re.findall(r"`([^`]+)`", row.group(2)):
            w = re.match(r"^\(?([a-z_][a-z0-9_]*)(?:\s|$|\))", tok)
            if w and re.match(r"^[a-z_][a-z0-9_]*\s*\[", tok) and w.group(1) not in mods[mod] and w.group(1) not in ("alloc_fn", "free_fn"):
                findings.append(f"AIPL_SPEC.md:{n}: the std/{mod} row names {w.group(1)}, which std/{mod} does not export")
    return findings


def check_seed():
    """bootstrap/README.md's current seed hash is the checked-in seed's."""
    import hashlib
    seed = os.path.join(ROOT, "bootstrap", "aipl-linux-x86_64")
    readme = open(os.path.join(ROOT, "bootstrap", "README.md"), encoding="utf-8").read()
    m = re.search(r"sha256 `([0-9a-f]{64})`", readme)
    if not m or not os.path.exists(seed):
        return ["bootstrap/README.md: no seed hash to check"]
    actual = hashlib.sha256(open(seed, "rb").read()).hexdigest()
    found = [] if actual == m.group(1) else [f"bootstrap/README.md: says the seed is sha256 {m.group(1)[:12]}..., it is {actual[:12]}..."]
    # the AArch64 seed's hash, on the line that says so
    arm = os.path.join(ROOT, "bootstrap", "aipl-linux-aarch64")
    a = re.search(r"AArch64 seed sha256 `([0-9a-f]{64})`", readme)
    if not a or not os.path.exists(arm):
        return found + ["bootstrap/README.md: no AArch64 seed hash to check"]
    actual = hashlib.sha256(open(arm, "rb").read()).hexdigest()
    return found if actual == a.group(1) else found + [f"bootstrap/README.md: says the AArch64 seed is sha256 {a.group(1)[:12]}..., it is {actual[:12]}..."]


def check_comments(files):
    """The `;;` comments of the library and the toolchain are documentation too
    (`aipl doc` shows them): none uses a form the language no longer has."""
    gone = re.compile(r"io\.(print_int|println_int|print_i64|println_i64|print_f64|println_f64)\b|\(loop [a-z_]+ |one type per function")
    findings = []
    for f in sorted(files):
        if not (f.startswith("aipl_src/") and f.endswith(".aipl")):
            continue
        for n, line in enumerate(open(os.path.join(ROOT, f), encoding="utf-8"), 1):
            if line.lstrip().startswith(";;") and gone.search(line) and not HISTORY_HINT.search(line):
                findings.append(f"{f}:{n}: a comment uses a form that is gone: {line.strip()[:80]}")
    return findings


def spec_sections(spec):
    """The section ids the spec's headings define: 4, 4.A, 6.5, 12.6."""
    ids, top = set(), None
    for line in spec.split("\n"):
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


def check_sections(files):
    """Every section of the spec that a document, a source comment, or a test
    names is one the spec has: "section 4.M" inside AIPL_SPEC.md, and
    "AIPL_SPEC.md 4.K" or "AIPL_SPEC.md section 6.5" anywhere."""
    spec = open(os.path.join(ROOT, "AIPL_SPEC.md"), encoding="utf-8").read()
    valid = spec_sections(spec)
    findings = []
    ids = r"\d+(?:\.[0-9A-Z])?(?:\.\d+)?"
    inside = re.compile(r"\b[Ss]ections? (%s(?:(?:,| and|, and) %s)*)" % (ids, ids))
    outside = re.compile(r"AIPL_SPEC\.md[`,]? (?:section |sections )?(%s(?:(?:,| and|, and) %s)*)" % (ids, ids))
    for f in sorted(files):
        if not f.endswith((".md", ".rs", ".aipl", ".py", ".sh")) or f.startswith(("docs/history/", "docs/notes/")):
            continue
        try:
            lines = open(os.path.join(ROOT, f), encoding="utf-8").read().split("\n")
        except (UnicodeDecodeError, FileNotFoundError):
            continue
        rx = [inside, outside] if f == "AIPL_SPEC.md" else [outside]
        for n, line in enumerate(lines, 1):
            for r in rx:
                for m in r.finditer(line):
                    for sid in re.findall(ids, m.group(1)):
                        if sid not in valid and sid.rsplit(".", 1)[0] not in ("",) and not (f == "AIPL_SPEC.md" and f"{sid}" in ("1", "2", "3")):
                            findings.append(f"{f}:{n}: names section {sid} of the spec, which has no such heading")
    return findings


def check_flags(docs):
    """Every --flag in the spec's command table is one a command has."""
    helps = run(aipl(), "--help")
    for c in re.findall(r"^  ([a-z-]+)\s", helps, re.M):
        helps += run(aipl(), c, "--help")
    helps += run(os.path.join(ROOT, "target", "release", "aipl-run"), "--help") if os.path.exists(os.path.join(ROOT, "target", "release", "aipl-run")) else ""
    known = set(re.findall(r"--[a-z][a-z-]+", helps)) | {"--help", "--version", "--sandbox", "--listen", "--serve-fresh", "--requests", "--name", "--temporary"}
    findings = []
    for n, line in enumerate(open(os.path.join(ROOT, "AIPL_SPEC.md"), encoding="utf-8"), 1):
        if re.match(r"^\| `aipl", line):
            for fl in sorted(set(re.findall(r"(?<![A-Za-z-])--[a-z][a-z-]+", line))):
                if fl not in known:
                    findings.append(f"AIPL_SPEC.md:{n}: the command table has {fl}, which no command takes")
    return findings


def check_forms():
    """Every surface form of the language is in the spec."""
    src = open(os.path.join(ROOT, "src/parser.rs"), encoding="utf-8").read()
    heads = re.findall(r'"([a-z0-9_.!]+)"', src.split("pub const HEAD_NAMES")[1].split("];")[0])
    spec = open(os.path.join(ROOT, "AIPL_SPEC.md"), encoding="utf-8").read()
    internal = {"mem.load_f32", "mem.load_f64", "mem.store_f32", "mem.store_f64",  # rejected, and the spec says so
                "counted_loop", "format.dbg", "format.eprintln", "format.expect", "format.piece", "format.print", "format.println", "format.str", "format.to"}
    return [f"AIPL_SPEC.md: the form {h} is not in the spec" for h in heads if h not in internal and not re.search(r"[(`| ]%s[ )`|]" % re.escape(h), spec)]


def main():
    show_all = "--all" in sys.argv
    files = set(f for f in tracked() if f)
    dirs = set()
    for f in files:
        d = os.path.dirname(f)
        while d:
            dirs.add(d)
            d = os.path.dirname(d)
    docs = sorted(f for f in files if f.endswith(".md"))
    findings = []
    for doc in docs:
        archive = doc.startswith(ARCHIVES)
        if archive and not show_all:
            continue
        # a plan not started, or only proposed, names files it would make
        head = "".join(open(os.path.join(ROOT, doc), encoding="utf-8").readlines()[:15])
        planned = "planned, not started" in head or "**Status: proposed" in head
        in_fence = False
        for n, line in enumerate(open(os.path.join(ROOT, doc), encoding="utf-8"), 1):
            if line.lstrip().startswith("```"):
                in_fence = not in_fence
            if in_fence:
                continue
            named = [m.group(1) for m in PATH.finditer(line) if m.group(1).startswith(KNOWN_ROOTS)]
            named += [g for g in LINK.findall(line) if not g.startswith(("http", "mailto"))]
            for p in named:
                if "<" in p or "{" in p or p.endswith((".wasm", ".exe")) or re.search(r"/(NN|TASK|K)[_./]", p) or "NN_" in p:
                    continue
                if planned or re.search(r"\b(Deleted|deleted|removed|gone|merged into|stays in the attic)\b", line):
                    continue
                if not exists(files, dirs, p, doc):
                    findings.append(f"{doc}:{n}: names {p}, which is not in the repository")
            if doc in FRONT and not in_fence:
                for rx, why in STALE:
                    if why and re.search(rx, line) and not HISTORY_HINT.search(line):
                        findings.append(f"{doc}:{n}: {why}: {line.strip()[:90]}")
    # numbers
    tests = sorted(f for f in files if re.match(r"tests/test_[a-z0-9_]+\.rs$", f))
    spec = open(os.path.join(ROOT, "AIPL_SPEC.md"), encoding="utf-8").read()
    m = re.search(r"Files today \((\d+) files", spec)
    if m and int(m.group(1)) != len(tests):
        findings.append(f"AIPL_SPEC.md: says {m.group(1)} test files; there are {len(tests)}")
    for t in tests:
        name = os.path.basename(t)
        if name not in spec:
            findings.append(f"AIPL_SPEC.md: its list of test files lacks {name}")
    std = sorted(os.path.basename(f)[:-5] for f in files if re.match(r"aipl_src/std/[a-z0-9_]+\.aipl$", f))
    for mod in std:
        if not re.search(r"\|\s*`%s`\s*\|" % re.escape(mod), spec):
            findings.append(f"AIPL_SPEC.md: the standard library table lacks std/{mod}")
    findings += check_seed()
    findings += check_comments(files)
    findings += check_names(files, docs, show_all)
    findings += check_sections(files)
    findings += check_flags(docs)
    findings += check_forms()
    for f in findings[:200]:
        print(f)
    print(f"{len(findings)} findings in {len([d for d in docs if show_all or not d.startswith(ARCHIVES)])} documents")
    sys.exit(min(len(findings), 100))


if __name__ == "__main__":
    main()
