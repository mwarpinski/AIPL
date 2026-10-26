#!/usr/bin/env python3
"""Rewrites the old ways of making a struct into the named forms
(docs/design/LANGUAGE_DECISIONS.md item 4):

    (let p:(ptr S) (new S a))          (let p:(ptr S) (new S a [x:1 y:(zero i32)]))
    (put p S.x 1)                ->
    (new S a)                    ->    (new S a [x:(zero i32) y:(zero i32)])
    (make V 1.0 2.0)             ->    (make V [x:1.0 y:2.0])        V a value struct

A `new` that a `let` or `set!` binds takes in the `put`s right after it,
while each sets the next field in declaration order, its value does not
read the new struct, and nothing but white space comes between them: the
named form makes the struct, then stores the values in the order written,
so everything happens in the order it did. Every field no
`put` sets is given its zero, `(zero T)`, the type as the struct's module
writes it, qualified for the file that makes it. A union's `make` is left
alone.

A struct is found by its name: unqualified, in the same program (a
`(module ...)` form); qualified, in the module the name or an import alias
names, preferring one in the same file. A site whose struct is not found,
or found twice with different fields, or whose zero names a module the
file does not import, is left alone and listed; in Markdown, only the
```lisp and bare ``` blocks are read; so is a `make` whose value
count is not the field count; `(call (make T) ...)`, a generic function
named make, is not a struct's. Every other byte of the file stays.

    python3 tools/named_fields.py [--write] FILE...
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from retire_loop import tokens, form_end  # noqa: E402

BUILTIN = {'i32', 'i64', 'f32', 'f64', 'bool', 'str', 'void', 'u8', 'i8', 'u16', 'i16',
           'ptr', 'arr', 'fn', 'result', '->', 'no_leak'}
WIDTH = 100


class Program:
    """One `(module NAME ...)` form: its structs, other type names, and imports."""

    def __init__(self, file, name, start, end):
        self.file, self.name, self.start, self.end = file, name, start, end
        self.structs = {}   # bare name -> Def
        self.types = set()  # every struct, enum, and union name it defines
        self.aliases = {}   # name written before a '.' -> module name


class Def:
    def __init__(self, prog, name, params, fields, value):
        self.prog, self.name, self.params, self.fields, self.value = prog, name, params, fields, value


class Source:
    def __init__(self, path):
        self.path = path
        self.text = open(path, errors='replace').read()
        self.rust = not path.endswith(('.aipl', '.md'))
        # in Markdown only the ```lisp and bare ``` blocks: prose has quotes
        # and parentheses of its own (blanked, so offsets stay)
        read = self.text
        if path.endswith('.md'):
            read = re.sub(r'(?s)(```(?:lisp|aipl)?\n.*?```)|[^\n]', lambda m: m.group(1) or ' ', self.text)
        self.toks = list(tokens(read, self.rust))
        self.progs = []

    def tok(self, k):
        return self.text[self.toks[k][1]:self.toks[k][2]]

    def span(self, a, b):
        """The text of tokens a..b-1."""
        return self.text[self.toks[a][1]:self.toks[b - 1][2]]

    def head(self, k):
        if k + 1 < len(self.toks) and self.toks[k][0] == '(' and self.toks[k + 1][0] == 'a':
            return self.tok(k + 1)
        return None

    def children(self, k):
        """(start, end) token ranges of the items of the form at k."""
        out, j, e = [], k + 1, form_end(self.toks, k) - 1
        while j < e:
            f = form_end(self.toks, j)
            out.append((j, f))
            j = f
        return out

    def prog_at(self, k):
        best = None
        for p in self.progs:
            if p.start <= k < p.end and (best is None or p.start > best.start):
                best = p
        return best


def norm(s):
    return re.sub(r'\s+', ' ', s).strip()


def read_fields(src, a, b):
    """[(name, type text)] of a field list's tokens a..b (inside its brackets)."""
    out, j = [], a
    while j < b:
        if src.toks[j][0] == '(':
            e = form_end(src.toks, j)
            out += read_fields(src, j + 2, e - 1)  # (pub x:T) / (read x:T)
            j = e
            continue
        t = src.tok(j)
        if t.endswith(':'):
            e = form_end(src.toks, j + 1)
            out.append((t[:-1], src.span(j + 1, e)))
            j = e
        elif ':' in t:
            n, ty = t.split(':', 1)
            out.append((n, ty))
            j += 1
        else:
            j += 1
    return out


def collect(src):
    for k in range(len(src.toks)):
        if src.head(k) == 'module' and k + 2 < len(src.toks) and src.toks[k + 2][0] == 'a':
            src.progs.append(Program(src, src.tok(k + 2), k, form_end(src.toks, k)))
    for p in src.progs:
        for k in range(p.start, p.end):
            h = src.head(k)
            if h == 'import':
                items = [src.span(a, b) for a, b in src.children(k)]
                if len(items) >= 2:
                    mod = items[1].split('/')[-1]
                    alias = items[3] if len(items) == 4 and items[2] == 'as' else mod
                    p.aliases[alias] = mod
            elif h in ('struct', 'enum', 'union') and src.prog_at(k) is p:
                ch = src.children(k)
                if len(ch) < 3:
                    continue
                na, nb = ch[1]
                params = []
                if src.toks[na][0] == 'a':
                    name = src.tok(na)
                elif src.toks[na][0] == '(':
                    sub = src.children(na)
                    name = src.span(*sub[0])
                    params = [src.span(*s) for s in sub[1:]]
                else:
                    continue
                p.types.add(name)
                if h != 'struct' or src.toks[ch[2][0]][0] != '[':
                    continue
                fa, fb = ch[2]
                fields = read_fields(src, fa + 1, fb - 1)
                value = any(norm(src.span(a, b)) == '(value)' for a, b in ch[3:])
                p.structs[name] = Def(p, name, params, fields, value)


class Index:
    def __init__(self, sources):
        self.sources = sources
        self.by_module = {}
        for s in sources:
            for p in s.progs:
                self.by_module.setdefault(p.name, []).append(p)

    def find(self, src, prog, written):
        """The Def a struct name written in prog names, or a reason."""
        if '.' not in written:
            d = prog.structs.get(written)
            return (d, None) if d else (None, f"no struct {written} in module {prog.name}")
        q, bare = written.rsplit('.', 1)
        mod = prog.aliases.get(q, q)
        if mod == prog.name:
            d = prog.structs.get(bare)
            return (d, None) if d else (None, f"no struct {written}")
        cands = [p.structs[bare] for p in self.by_module.get(mod, []) if bare in p.structs]
        if not cands:
            return None, f"no struct {written} (module {mod})"
        local = [d for d in cands if d.prog.file is src]
        pick = local or cands
        if len({tuple(d.fields) for d in pick}) > 1:
            return None, f"{written} is defined differently in {len(pick)} places"
        return pick[0], None


def qualify(ty, d, site, subst):
    """Type text `ty` of a field of d, as `site` writes it; None and the
    module when site does not import a module the type names."""
    out, need = [], None
    for m in re.finditer(r'[^\s()\[\]]+|[\s()\[\]]+', ty):
        w = m.group(0)
        if not re.match(r'[^\s()\[\]]', w) or w in BUILTIN:
            out.append(w)
            continue
        if w in subst:
            out.append(subst[w])
            continue
        if '.' in w:
            q, bare = w.rsplit('.', 1)
            mod = d.prog.aliases.get(q, q)
        elif w in d.prog.types:
            mod, bare = d.prog.name, w
        else:
            out.append(w)  # a type parameter of the site's own template, or unknown
            continue
        if mod == site.name:
            out.append(bare)
            continue
        names = [a for a, mm in site.aliases.items() if mm == mod]
        if not names:
            need = mod
            out.append(f"{mod}.{bare}")
            continue
        out.append(f"{names[0]}.{bare}")
    return ''.join(out), need


def struct_of(src, prog, index, a, b):
    """The Def, its written form, and the type-parameter substitution for a
    struct written at tokens a..b ((Box i32) or S)."""
    if src.toks[a][0] == 'a':
        d, why = index.find(src, prog, src.tok(a))
        return d, why, {}
    if src.toks[a][0] != '(':
        return None, 'not a struct', {}
    sub = src.children(a)
    d, why = index.find(src, prog, src.span(*sub[0]))
    if not d:
        return None, why, {}
    args = [src.span(*s) for s in sub[1:]]
    if len(args) != len(d.params):
        return None, 'wrong type argument count', {}
    return d, None, dict(zip(d.params, args))


def named_list(pairs, col, rust):
    """[x:v ...], one field a line past WIDTH, never inside a Rust string."""
    one = '[' + ' '.join(f"{n}:{v}" for n, v in pairs) + ']'
    if rust or col + len(one) <= WIDTH or '\n' in one:
        return one
    pad = '\n' + ' ' * (col + 1)
    return '[' + pad.join(f"{n}:{v}" for n, v in pairs) + ']'


def column(text, at):
    return at - (text.rfind('\n', 0, at) + 1)


def rewrite(src, index, report):
    edits = []  # (start, end, text)
    done_new = set()
    for k in range(len(src.toks)):
        h = src.head(k)
        prog = src.prog_at(k)
        if prog is None:
            continue
        if h in ('let', 'set!'):
            ch = src.children(k)
            if len(ch) < 3:
                continue
            na, nb = ch[1]
            name = src.tok(na).split(':', 1)[0]
            va, vb = ch[-1]
            if src.head(va) == 'new' and len(src.children(va)) == 3:
                done_new.add(va)
                fold_new(src, index, prog, va, name, form_end(src.toks, k), edits, report)
        elif h == 'new' and k not in done_new and len(src.children(k)) == 3:
            fold_new(src, index, prog, k, None, None, edits, report)
        elif h == 'make':
            ch = src.children(k)
            # (call (make T) ...): a generic function named make (std/vec's), not a struct
            if len(ch) < 2 or (k >= 2 and src.toks[k - 2][0] == '(' and src.tok(k - 1) in ('call', 'ref')):
                continue
            a, b = ch[1]
            if src.toks[a][0] == '[' or (len(ch) == 3 and src.toks[ch[2][0]][0] == '['):
                continue
            d, why, subst = struct_of(src, prog, index, a, b)
            if not d or not d.value:
                continue
            vals = [src.span(x, y) for x, y in ch[2:]]
            if len(vals) != len(d.fields):
                report.append((src.path, src.toks[k][1], f"make of {src.span(a, b)}: {len(vals)} values for {len(d.fields)} fields"))
                continue
            start = src.toks[ch[2][0]][1]
            end = src.toks[ch[-1][1] - 1][2]
            edits.append((start, end, named_list([(f[0], v) for f, v in zip(d.fields, vals)], column(src.text, start), src.rust)))
    return edits


def fold_new(src, index, prog, k, var, after, edits, report):
    ch = src.children(k)
    sa, sb = ch[1]
    written = src.span(sa, sb)
    d, why, subst = struct_of(src, prog, index, sa, sb)
    where = src.toks[k][1]
    if not d:
        report.append((src.path, where, f"(new {written} ...): {why}"))
        return
    if d.value:
        report.append((src.path, where, f"(new {written} ...): a value struct"))
        return
    order = [f[0] for f in d.fields]
    given, removes = {}, []
    if var is not None:
        j, prev_end, last = after, src.toks[after - 1][2], -1
        while j < len(src.toks) and src.head(j) == 'put':
            pc = src.children(j)
            if len(pc) < 4 or src.span(*pc[1]) != var:
                break
            if src.toks[sa][0] == 'a':
                path = src.tok(pc[2][0])
                if len(pc) != 4 or '.' not in path or path.rsplit('.', 1)[0] != written:
                    break
                field, val = path.rsplit('.', 1)[1], pc[3]
            else:
                if len(pc) != 5 or norm(src.span(*pc[2])) != norm(written):
                    break
                field, val = src.tok(pc[3][0]), pc[4]
            if field not in order or order.index(field) <= last:
                break
            vtext = src.span(*val)
            if any(src.toks[t][0] == 'a' and src.tok(t) == var for t in range(*val)):
                break
            gap = src.text[prev_end:src.toks[j][1]]
            if (gap.replace('\\n', ' ').replace('\\t', ' ').strip()) != '':
                break
            last = order.index(field)
            given[field] = vtext
            jend = form_end(src.toks, j)
            removes.append((prev_end, src.toks[jend - 1][2]))
            prev_end = src.toks[jend - 1][2]
            j = jend
    pairs = []
    for n, ty in d.fields:
        if n in given:
            pairs.append((n, given[n]))
            continue
        q, need = qualify(ty, d, prog, subst)
        if need:
            report.append((src.path, where, f"(new {written} ...): the zero of field {n} names module {need}, which this file does not import"))
            return
        pairs.append((n, f"(zero {q})"))
    end = src.toks[ch[2][1] - 1][2]
    edits.append((end, end, ' ' + named_list(pairs, column(src.text, end) + 1, src.rust)))
    edits.extend((a, b, '') for a, b in removes)


def main():
    args = sys.argv[1:]
    write = '--write' in args
    paths = [a for a in args if a != '--write']
    sources = []
    for p in paths:
        s = Source(p)
        collect(s)
        sources.append(s)
    index = Index(sources)
    total, report = 0, []
    for s in sources:
        edits = rewrite(s, index, report)
        if not edits:
            continue
        total += len(edits)
        text = s.text
        for a, b, t in sorted(edits, key=lambda e: (e[0], e[1]), reverse=True):
            text = text[:a] + t + text[b:]
        if write:
            open(s.path, 'w').write(text)
        else:
            print(f"{s.path}: {len(edits)} edits")
    for path, at, why in report:
        src = next(s for s in sources if s.path == path)
        line = src.text.count('\n', 0, at) + 1
        print(f"left alone: {path}:{line}: {why}")
    print(f"{total} edits{' written' if write else ''}, {len(report)} left alone")


if __name__ == '__main__':
    main()
