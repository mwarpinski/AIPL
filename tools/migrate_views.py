"""MEMORY_PLAN.md M1 (2026-10-19): (ptr str.Bytes) became the value
str.Bytes. This rewrote the repository, and rewrites any AIPL code written
before it (a file, or every file under a directory):

    python3 tools/migrate_views.py PATH

Rewrites, in .aipl, .rs, .md, and .py files (skipping target/ and
bootstrap/):
  (ptr str.Bytes)                     -> str.Bytes
  (get X str.Bytes.addr)              -> (call str.addr X)    (X any expression)
  (ptr.null str.Bytes)                -> (call str.absent)    (a view is a value)
  (neq X (call str.absent))           -> (call str.present X)
  (eq X (call str.absent))            -> (not (call str.present X))
and lists what needs a person: str.repoint (make a new view instead),
(put X str.Bytes...), (new str.Bytes).
"""
import os, re, sys

root = sys.argv[1]
# a single file is a directory of one
single = os.path.isfile(root)
skip_dirs = {"target", "bootstrap", ".git"}

def end_of(text, k):
    # the index just past the expression starting at k
    if text[k] == "(":
        d = 0
        while True:
            if text[k] == "(":
                d += 1
            elif text[k] == ")":
                d -= 1
                if d == 0:
                    return k + 1
            k += 1
    while text[k] not in " )\n":
        k += 1
    return k

def nulls(t):
    t = t.replace("(ptr.null str.Bytes)", "(call str.absent)")
    for op, wrap in (("neq", "(call str.present {x})"), ("eq", "(not (call str.present {x}))")):
        key = f"({op} "
        out, i = [], 0
        while True:
            j = t.find(key, i)
            if j < 0:
                out.append(t[i:])
                break
            k = j + len(key)
            e = end_of(t, k)
            if t[e:].startswith(" (call str.absent))"):
                out.append(t[i:j])
                out.append(wrap.format(x=t[k:e]))
                i = e + len(" (call str.absent))")
            else:
                out.append(t[i:k])
                i = k
        t = "".join(out)
    return t

def get_addr(text):
    # (get X str.Bytes.addr) with X a balanced expression
    out, i = [], 0
    key = "(get "
    while True:
        j = text.find(key, i)
        if j < 0:
            out.append(text[i:])
            return "".join(out)
        # parse X
        k = j + len(key)
        depth, start = 0, k
        if k < len(text) and text[k] == "(":
            while k < len(text):
                if text[k] == "(":
                    depth += 1
                elif text[k] == ")":
                    depth -= 1
                    if depth == 0:
                        k += 1
                        break
                k += 1
        else:
            while k < len(text) and text[k] not in " )\n":
                k += 1
        x = text[start:k]
        rest = text[k:]
        m = re.match(r" str\.Bytes\.addr\)", rest)
        if m:
            out.append(text[i:j])
            out.append(f"(call str.addr {x})")
            i = k + m.end()
        else:
            out.append(text[i:j + len(key)])
            i = j + len(key)

changed, todo = 0, []
walk = [(os.path.dirname(root) or ".", [], [os.path.basename(root)])] if single else os.walk(root)
for dirpath, dirs, files in walk:
    dirs[:] = [d for d in dirs if d not in skip_dirs]
    for f in files:
        if not f.endswith((".aipl", ".rs", ".md", ".py")):
            continue
        p = os.path.join(dirpath, f)
        if p.endswith("migrate_views.py"):
            continue
        t = open(p, encoding="utf-8").read()
        n = t.replace("(ptr str.Bytes)", "str.Bytes")
        n = get_addr(n)
        if f.endswith(".aipl"):
            n = nulls(n)
        for pat in ("str.repoint", "(put ", "(new str.Bytes)"):
            for m in re.finditer(re.escape(pat), n):
                line = n.count("\n", 0, m.start()) + 1
                ln = n.splitlines()[line - 1]
                if pat == "(put " and "str.Bytes." not in ln:
                    continue
                todo.append(f"{p}:{line}: {ln.strip()[:110]}")
        if n != t:
            open(p, "w", encoding="utf-8").write(n)
            changed += 1
print(f"{changed} files changed")
print("\n".join(todo))
