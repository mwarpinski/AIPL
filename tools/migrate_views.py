"""MEMORY_PLAN.md M1 (2026-10-19): (ptr str.Bytes) became the value
str.Bytes. This rewrote the repository, and rewrites any AIPL code written
before it (a file, or every file under a directory):

    python3 tools/migrate_views.py PATH

Rewrites, in .aipl, .rs, .md, and .py files under ROOT (skipping target/
and bootstrap/):
  (ptr str.Bytes)            -> str.Bytes
  (get X str.Bytes.addr)     -> (call str.addr X)     (X any expression)
and lists what needs a person: (ptr.null str.Bytes) (a view is a
value: use str.absent and str.present), str.repoint (make a new view
instead), (put X str.Bytes...), (new str.Bytes).
"""
import os, re, sys

root = sys.argv[1]
# a single file is a directory of one
single = os.path.isfile(root)
skip_dirs = {"target", "bootstrap", ".git"}

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
        for pat in ("(ptr.null str.Bytes)", "str.repoint", "(put ", "(new str.Bytes)"):
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
