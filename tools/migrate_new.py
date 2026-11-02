#!/usr/bin/env python3
"""Every allocation names its allocator (docs/design/MEMORY_PLAN.md F,
decision 5): rewrite (new T) and (arr.new T n), memory that is never
freed, to the allocator that says so.

  (new T)       -> (call (alloc.create T) (call alloc.forever))
  (arr.new T n) -> (call (alloc.array T) (call alloc.forever) n)

and add (import alloc) to a module that does not have it. std/alloc
itself writes (create T) and (forever); std/gen, std/heap, and std/arena,
which std/alloc is made of and so cannot import it, use gen.forever and
gen.forever_array, the same memory.

Files: .aipl (comments and strings left alone), .md (only ```lisp
blocks), and .rs (AIPL programs written inside Rust strings; an import is
added on the (module NAME line itself, so line numbers in expected
messages do not move).

Run once: python3 tools/migrate_new.py FILE...
"""
import re
import sys

LOW = {"gen", "heap", "arena"}


def skip_to_end_of_form(src, i):
    """The index just past the balanced form or atom starting at i."""
    if src[i] == "(":
        depth = 0
        j = i
        while j < len(src):
            c = src[j]
            if c == '"':
                j += 1
                while src[j] != '"':
                    j += 2 if src[j] == "\\" else 1
            elif c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    return j + 1
            j += 1
        raise ValueError("unbalanced")
    j = i
    while j < len(src) and not src[j].isspace() and src[j] not in "()":
        j += 1
    return j


def rewrite_code(src, code_ranges):
    """src with every new / arr.new inside code_ranges rewritten, and the
    positions (in the new text) of the modules that now use alloc."""
    out, at, out_len = [], 0, 0
    modules = []   # [name, start in the new text, uses alloc]
    i = 0
    n = len(src)

    def in_code(k):
        return any(a <= k < b for a, b in code_ranges)

    while i < n:
        if not in_code(i):
            i += 1
            continue
        c = src[i]
        if c == ";" and src_is_aipl:
            while i < n and src[i] != "\n":
                i += 1
            continue
        if c == '"' and src_is_aipl:
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
            i += 1
            continue
        mm = re.match(r"\(module ([\w.]+)", src[i:])
        if mm:
            modules.append([mm.group(1), out_len + (i - at), False])
            i += mm.end()
            continue
        m = re.match(r"\((new|arr\.new)\s+", src[i:])
        if m:
            kind = m.group(1)
            t0 = i + m.end()
            t1 = skip_to_end_of_form(src, t0)
            ty = src[t0:t1]
            k = t1
            size = ""
            if kind == "arr.new":
                while src[k].isspace():
                    k += 1
                e1 = skip_to_end_of_form(src, k)
                size = src[k:e1]
                k = e1
            while src[k].isspace():
                k += 1
            if src[k] != ")":
                i += 1
                continue
            mod = modules[-1][0] if modules else ""
            if mod == "alloc":
                new = f"(call (create {ty}) (call forever))" if kind == "new" else f"(call (array {ty}) (call forever) {size})"
            elif mod == "gen":
                new = f"(call (forever {ty}))" if kind == "new" else f"(call (forever_array {ty}) {size})"
            elif mod in LOW:
                new = f"(call (gen.forever {ty}))" if kind == "new" else f"(call (gen.forever_array {ty}) {size})"
            else:
                new = f"(call (alloc.create {ty}) (call alloc.forever))" if kind == "new" else f"(call (alloc.array {ty}) (call alloc.forever) {size})"
                if modules:
                    modules[-1][2] = True
            piece = src[at:i]
            out.append(piece)
            out.append(new)
            out_len += len(piece) + len(new)
            at = k + 1
            i = k + 1
            continue
        i += 1
    out.append(src[at:])
    return "".join(out), [(name, pos) for name, pos, uses in modules if uses]


def add_imports(src, users, same_line):
    """(import alloc) in each module at the positions `users` that lacks it,
    the last first so the earlier positions stay right."""
    for name, pos in sorted(users, key=lambda u: -u[1]):
        head = "(module " + name
        assert src.startswith(head, pos), (name, src[pos:pos + 40])
        end_head = pos + len(head)
        nxt = src.find("(module ", end_head)
        body = src[end_head:nxt if nxt >= 0 else len(src)]
        if "(import alloc)" in body:
            continue
        if same_line:
            src = src[:end_head] + " (import alloc)" + src[end_head:]
            continue
        imp = re.search(r"\n([ \t]*)\(import ", body)
        if imp:
            k = end_head + imp.start() + 1
            src = src[:k] + imp.group(1) + "(import alloc)\n" + src[k:]
        else:
            eol = src.find("\n", end_head)
            src = src[:eol + 1] + "  (import alloc)\n" + src[eol + 1:]
    return src


src_is_aipl = True


def main():
    global src_is_aipl
    for p in sys.argv[1:]:
        src = open(p).read()
        if p.endswith(".md"):
            src_is_aipl = True
            ranges = [(m.start(1), m.end(1)) for m in re.finditer(r"```lisp\n(.*?)```", src, re.S)]
            same_line = False
        elif p.endswith(".rs"):
            src_is_aipl = False
            ranges = [(0, len(src))]
            same_line = True
        else:
            src_is_aipl = True
            ranges = [(0, len(src))]
            same_line = False
        new, users = rewrite_code(src, ranges)
        new = add_imports(new, users, same_line)
        if new != src:
            open(p, "w").write(new)
            print(f"rewrote {p}", file=sys.stderr)


if __name__ == "__main__" and not (len(sys.argv) > 1 and sys.argv[1] == "--short"):
    main()


# ---- step 3: the short form, (new T a) and (arr.new T a n) ----
#
# Once new and arr.new name their allocator (MEMORY_PLAN.md F, decided
# 2026-10-19), what step 2 wrote reads better as the form again:
#
#   (call (alloc.create T) a)   -> (new T a)
#   (call (alloc.array T) a n)  -> (arr.new T a n)
#
# python3 tools/migrate_new.py --short FILE...  (same file kinds as above)

def shorten(src, code_ranges, aipl):
    """src with each step-2 call inside code_ranges back in the short form,
    inner ones first."""
    out, at, i, n = [], 0, 0, len(src)

    def in_code(k):
        return any(a <= k < b for a, b in code_ranges)

    while i < n:
        if not in_code(i):
            i += 1
            continue
        c = src[i]
        if aipl and c == ";":
            while i < n and src[i] != "\n":
                i += 1
            continue
        if aipl and c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
            i += 1
            continue
        m = re.match(r"\(call \(alloc\.(create|array) ", src[i:])
        if m:
            t0 = i + m.end()
            t1 = skip_to_end_of_form(src, t0)
            k = t1
            while src[k].isspace():
                k += 1
            if src[k] == ")":
                end = skip_to_end_of_form(src, i)
                inner = lambda t: shorten(t, [(0, len(t))], aipl)
                rest = inner(src[k + 1:end - 1].strip())
                form = "new" if m.group(1) == "create" else "arr.new"
                out.append(src[at:i])
                out.append(f"({form} {inner(src[t0:t1])} {rest})")
                at = i = end
                continue
        i += 1
    out.append(src[at:])
    return "".join(out)


def main_short(paths):
    for p in paths:
        src = open(p).read()
        if p.endswith(".md"):
            ranges = [(m.start(1), m.end(1)) for m in re.finditer(r"```lisp\n(.*?)```", src, re.S)]
            new = shorten(src, ranges, True)
        else:
            new = shorten(src, [(0, len(src))], not p.endswith(".rs"))
        if new != src:
            open(p, "w").write(new)
            print(f"shortened {p}", file=sys.stderr)


if __name__ == "__main__" and len(sys.argv) > 1 and sys.argv[1] == "--short":
    main_short(sys.argv[2:])
    sys.exit(0)
