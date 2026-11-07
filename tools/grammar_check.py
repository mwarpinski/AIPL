#!/usr/bin/env python3
"""Checks that the grammar in AIPL_SPEC.md describes the programs that exist
(docs/LAUNCH_CHECKLIST.md, section 2: "Test the grammar so it cannot drift again").

    python3 tools/grammar_check.py [--verbose] [FILE.aipl ...]

1. Reads the EBNF out of AIPL_SPEC.md (the code blocks tagged `ebnf`).
2. Reads every program in the repository (every .aipl file, and every complete module written in a code block of the
   documentation) with a lexer that is written out here from the spec's section 2, and recognises it by the grammar.
   It uses nothing of either compiler.
3. Checks that the examples the grammar must refuse (NEGATIVE below) are refused.
4. Checks that every form the checker names (the heads in the spec's "Forms" tables) is in the grammar, and every
   terminal in the grammar is a form, an operator, or a type the language has: the grammar names nothing that is gone.

Prints one line per finding, with the position of the first token the grammar could not place and what it wanted
there, and exits with their count (capped at 100).
"""
import os
import re
import sys
import threading

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = os.path.join(ROOT, "AIPL_SPEC.md")

# Files that are not programs: they are written to fail, so the reader of the grammar is not expected to take them.
NOT_PROGRAMS = {
}

# Examples the grammar must refuse, each with the reason.
NEGATIVE = [
    ("(module m (fn f [] -> i32 (let x 5) x))", "a let names its type"),
    ("(module m (fn f [] -> i32 (if true 1)))", "an if has two branches"),
    ("(module m (fn f [] -> i32 (+ 1 2)", "an unclosed form"),
    ("(module m (fn f [x] -> i32 x))", "a parameter has a type"),
    ("(module m (fn f [] i32 1))", "a function names its result after ->"),
    ("(module m (fn f [] -> i32 (frobnicate 1)))", "an unknown head"),
    ("(module m (fn f [] -> i32 (call (+ 1 2))))", "call takes a name"),
    ("(module m (fn f [] -> i32 (match_result 1 (ok x x))))", "match_result has both arms"),
    ("(module m (fn f [] -> i32 (cond (true 1))))", "a cond ends with else"),
    ("(module m (fn f [] -> i32 (set! 5 1)))", "set! names a variable"),
]


# ---------------------------------------------------------------- the lexer (AIPL_SPEC.md, section 2, "Lexical rules")

INT = re.compile(r"[+-]?(0x[0-9a-fA-F]+(_[0-9a-fA-F]+)*|0b[01]+(_[01]+)*|[0-9]+)\Z")
INT64 = re.compile(r"[+-]?(0x[0-9a-fA-F]+(_[0-9a-fA-F]+)*|0b[01]+(_[01]+)*|[0-9]+)i64\Z")
FLOAT = re.compile(r"[+-]?([0-9]+\.[0-9]*|\.[0-9]+)([eE][+-]?[0-9]+)?\Z")
FLOAT32 = re.compile(r"[+-]?([0-9]+\.[0-9]*|\.[0-9]+)([eE][+-]?[0-9]+)?f32\Z")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_.\-]*\Z")
PATH = re.compile(r"[A-Za-z_][A-Za-z0-9_./\-]*\Z")


def tokenize(text):
    """[(atom_kind, text, line, col)]; raises ValueError on what is not a token."""
    toks, i, n, line, col = [], 0, len(text), 1, 1
    while i < n:
        ch = text[i]
        if ch == "\n":
            i += 1; line += 1; col = 1; continue
        if ch in " \t\r":
            i += 1; col += 1; continue
        if ch == ";":
            while i < n and text[i] != "\n":
                i += 1
            continue
        if ch in "()[]:":
            toks.append((ch, ch, line, col)); i += 1; col += 1; continue
        if ch == '"':
            j = i + 1
            while j < n and text[j] != '"':
                if text[j] == "\n":
                    raise ValueError(f"{line}:{col}: unterminated string")
                j += 2 if text[j] == "\\" else 1
            if j >= n:
                raise ValueError(f"{line}:{col}: unterminated string")
            toks.append(("string", text[i:j + 1], line, col))
            col += j + 1 - i; i = j + 1; continue
        j = i
        while j < n and text[j] not in " \t\r\n()[]:":
            j += 1
        s = text[i:j]
        if s == "->":
            kind = "->"
        elif s in ("true", "false"):
            kind = "boolean"
        elif INT.match(s):
            kind = "integer"
        elif INT64.match(s):
            kind = "integer64"
        elif FLOAT32.match(s):
            kind = "float32"
        elif FLOAT.match(s):
            kind = "float"
        else:
            kind = "symbol"
        toks.append((kind, s, line, col))
        col += j - i; i = j
    return toks


def atoms(tok):
    """The names a token answers to in the grammar: its text (as a terminal) and its classes."""
    kind, s = tok[0], tok[1]
    out = ["t:" + s] if kind in ("symbol", "(", ")", "[", "]", ":", "->", "boolean") else []
    if kind == "symbol":
        if PATH.match(s):
            out.append("c:path")
        if IDENT.match(s):
            out.append("c:identifier")
            if s[0].isupper():
                out.append("c:type_param")
    elif kind in ("integer", "integer64", "float", "float32", "string", "boolean"):
        out.append("c:" + kind)
    return out


# ---------------------------------------------------------------- the EBNF reader

LEXICAL = {"path", "identifier", "type_param", "integer", "integer64", "float", "float32", "string", "boolean"}


def spec_ebnf(path=SPEC):
    text = open(path, encoding="utf-8").read()
    blocks = re.findall(r"```ebnf\n(.*?)```", text, re.S)
    return "\n".join(blocks)


def ebnf_tokens(src):
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if c.isspace():
            i += 1
        elif src.startswith("(*", i):
            j = src.index("*)", i + 2)
            i = j + 2
        elif c == '"':
            j = src.index('"', i + 1)
            out.append(("t", src[i + 1:j])); i = j + 1
        elif c.isalpha() or c == "_":
            j = i
            while j < n and (src[j].isalnum() or src[j] == "_"):
                j += 1
            out.append(("n", src[i:j])); i = j
        elif src.startswith("::=", i):
            out.append(("::=", "::=")); i += 3
        elif c in "|()[]*+?;":
            out.append((c, c)); i += 1
        else:
            raise ValueError(f"grammar: unexpected {c!r} near {src[max(0, i - 30):i + 30]!r}")
    return out


class Grammar:
    def __init__(self, src):
        self.rules = {}
        self.order = []
        self.toks = ebnf_tokens(src)
        self.p = 0
        while self.p < len(self.toks):
            name = self.expect("n")
            self.expect("::=")
            body = self.alt()
            self.expect(";")
            if name in self.rules:
                raise ValueError(f"grammar: {name} is defined twice")
            self.rules[name] = body
            self.order.append(name)
        for name, body in self.rules.items():
            for ref in self.refs(body):
                if ref not in self.rules and ref not in LEXICAL:
                    raise ValueError(f"grammar: {name} uses {ref}, which is not defined")

    def peek(self):
        return self.toks[self.p][0] if self.p < len(self.toks) else None

    def expect(self, kind):
        if self.peek() != kind:
            raise ValueError(f"grammar: expected {kind}, got {self.toks[self.p] if self.p < len(self.toks) else 'end'}")
        self.p += 1
        return self.toks[self.p - 1][1]

    def alt(self):
        alts = [self.seq()]
        while self.peek() == "|":
            self.p += 1
            alts.append(self.seq())
        return alts[0] if len(alts) == 1 else ("alt", alts)

    def seq(self):
        items = []
        while self.peek() in ("t", "n", "(", "["):
            items.append(self.postfix())
        return items[0] if len(items) == 1 else ("seq", items)

    def postfix(self):
        k = self.peek()
        if k == "t":
            node = ("t", self.expect("t"))
        elif k == "n":
            name = self.expect("n")
            node = ("c", name) if name in LEXICAL else ("r", name)
        elif k == "(":
            self.p += 1
            node = self.alt()
            self.expect(")")
        else:
            self.p += 1
            node = ("opt", self.alt())
            self.expect("]")
        while self.peek() in ("*", "+", "?"):
            m = self.expect(self.peek())
            node = ({"*": "star", "+": "plus", "?": "opt"}[m], node)
        return node

    def refs(self, node):
        if node[0] == "r":
            yield node[1]
        elif node[0] in ("seq", "alt"):
            for x in node[1]:
                yield from self.refs(x)
        elif node[0] in ("opt", "star", "plus"):
            yield from self.refs(node[1])

    def terminals(self):
        out = set()
        def walk(node):
            if node[0] == "t":
                out.add(node[1])
            elif node[0] in ("seq", "alt"):
                for x in node[1]:
                    walk(x)
            elif node[0] in ("opt", "star", "plus"):
                walk(node[1])
        for body in self.rules.values():
            walk(body)
        return out


# ---------------------------------------------------------------- the recogniser

class Recogniser:
    """All the ways a rule can end at each position, memoised: polynomial, however ambiguous the grammar is."""

    def __init__(self, grammar):
        self.g = grammar
        self.first = self.first_sets()

    K = 2

    def ev(self, n, sets):
        """(the first atoms of what `n` matches, up to K of them, and whether it can match nothing)."""
        k = n[0]
        if k == "t":
            return {("t:" + n[1],)}, False
        if k == "c":
            return {("c:" + n[1],)}, False
        if k == "r":
            return sets[n[1]]
        if k == "seq":
            acc, nulls = {()}, True
            for x in n[1]:
                s, nu = self.ev(x, sets)
                new = set()
                for a in acc:
                    if len(a) >= self.K:
                        new.add(a)
                    else:
                        for b in s:
                            new.add((a + b)[: self.K])
                if nu:
                    new |= acc
                acc = new
                nulls = nulls and nu
            return {t for t in acc if t}, nulls
        if k == "alt":
            res, nul = set(), False
            for x in n[1]:
                s, nu = self.ev(x, sets)
                res |= s
                nul = nul or nu
            return res, nul
        if k in ("opt", "star"):
            s, _ = self.ev(n[1], sets)
            return s, True
        if k == "plus":
            return self.ev(n[1], sets)
        raise AssertionError(k)

    def first_sets(self):
        sets = {r: (set(), False) for r in self.g.rules}
        changed = True
        while changed:
            changed = False
            for r, body in self.g.rules.items():
                got = self.ev(body, sets)
                if got != sets[r]:
                    sets[r] = got
                    changed = True
        return sets

    def node_first(self, node):
        """(index, nullable): index maps a first atom to the set of second atoms (None: the match may end there)."""
        cache = self.__dict__.setdefault("_nf", {})
        key = id(node)
        if key not in cache:
            tuples, nullable = self.ev(node, self.first)
            index = {}
            for t in tuples:
                index.setdefault(t[0], set()).add(t[1] if len(t) > 1 else None)
            cache[key] = (index, nullable)
        return cache[key]

    def recognise(self, toks, start):
        self.toks = toks
        self.atoms = [atoms(t) for t in toks] + [[]]
        self.memo = {}
        self.far = -1
        self.want = set()
        ends = self.m(("r", start), 0)
        return len(toks) in ends

    def can_start(self, node, pos):
        index, nullable = self.node_first(node)
        if nullable:
            return True
        a2 = self.atoms[pos + 1] if pos + 1 < len(self.atoms) else []
        for a in self.atoms[pos]:
            seconds = index.get(a)
            if seconds is None:
                continue
            if None in seconds or not a2 or any(b in seconds for b in a2):
                return True
        return False

    def fail(self, pos, what):
        if pos > self.far:
            self.far, self.want = pos, set()
        if pos == self.far:
            self.want.add(what)

    def m(self, node, pos):
        k = node[0]
        if k == "t":
            if "t:" + node[1] in self.atoms[pos]:
                return (pos + 1,)
            self.fail(pos, '"' + node[1] + '"')
            return ()
        if k == "c":
            if "c:" + node[1] in self.atoms[pos]:
                return (pos + 1,)
            self.fail(pos, node[1])
            return ()
        key = (id(node), pos)
        hit = self.memo.get(key)
        if hit is not None:
            return hit
        if k == "r":
            res = self.m(self.g.rules[node[1]], pos) if self.can_start(self.g.rules[node[1]], pos) else self.refuse(node, pos)
        elif k == "seq":
            cur = {pos}
            for x in node[1]:
                nxt = set()
                for p in cur:
                    nxt.update(self.m(x, p))
                cur = nxt
                if not cur:
                    break
            res = tuple(sorted(cur))
        elif k == "alt":
            out = set()
            for x in node[1]:
                if self.can_start(x, pos):
                    out.update(self.m(x, pos))
                else:
                    self.refuse(x, pos)
            res = tuple(sorted(out))
        elif k == "opt":
            out = {pos}
            out.update(self.m(node[1], pos))
            res = tuple(sorted(out))
        elif k in ("star", "plus"):
            out = set() if k == "plus" else {pos}
            frontier = {pos}
            seen = set()
            while frontier:
                nxt = set()
                for p in frontier:
                    for e in self.m(node[1], p):
                        if e != p and e not in seen:
                            seen.add(e); nxt.add(e)
                out |= nxt
                frontier = nxt
            res = tuple(sorted(out))
        else:
            raise AssertionError(k)
        self.memo[key] = res
        return res

    def refuse(self, node, pos):
        index, _ = self.node_first(node)
        for a in index:
            self.fail(pos, a[2:] if a.startswith("c:") else '"' + a[2:] + '"')
        return ()


# ---------------------------------------------------------------- the corpus

def program_files():
    skip = {"target", ".git", "node_modules"}
    for d, dirs, files in os.walk(ROOT):
        dirs[:] = sorted(x for x in dirs if x not in skip)
        for f in sorted(files):
            if f.endswith(".aipl"):
                yield os.path.relpath(os.path.join(d, f), ROOT)


DOCS = ["AIPL_SPEC.md", "README.md", "PROMPT_GUIDE_FOR_AIS.md", "docs/FAQ.md", "docs/TOUR.md", "docs/USING_AIPL.md",
        "docs/LANGUAGE_VERSIONING.md", "docs/LIMITS.md", "ROADMAP.md", "DEVELOPING.md"]


def doc_modules():
    """(label, text) of each complete module in the documentation's code blocks."""
    for rel in DOCS:
        path = os.path.join(ROOT, rel)
        if not os.path.exists(path):
            continue
        text = open(path, encoding="utf-8").read()
        for m in re.finditer(r"```(?:lisp|aipl)\n(.*?)```", text, re.S):
            code = m.group(1)
            if code.lstrip().startswith("(module"):
                line = text.count("\n", 0, m.start()) + 2
                yield f"{rel}:{line}", code


def explain(rec, toks):
    if rec.far >= len(toks):
        return "unexpected end of the program"
    t = toks[rec.far]
    want = sorted(rec.want)
    shown = ", ".join(want[:8]) + (", ..." if len(want) > 8 else "")
    return f"{t[2]}:{t[3]}: at '{t[1]}': the grammar wants {shown}"


def check(rec, label, text, findings, verbose):
    try:
        toks = tokenize(text)
    except ValueError as e:
        findings.append(f"{label}: {e}")
        return
    if not rec.recognise(toks, "program"):
        findings.append(f"{label}:{explain(rec, toks)}")
    elif verbose:
        print(f"ok {label}")


def forms_in_spec(g):
    """Every head the spec's own forms tables name must be a terminal of the grammar."""
    text = open(SPEC, encoding="utf-8").read()
    text = text.split("Pitfalls for Code Generators")[0]    # the pitfalls table shows forms that are refused
    terminals = g.terminals()
    out = []
    for m in re.finditer(r"^\| `\((?!module )([a-z][a-z0-9_.!]*)[ )]", text, re.M):
        head = m.group(1)
        if head not in terminals and head not in IN_TABLES_NOT_FORMS:
            out.append(head)
    return sorted(set(out))


# heads that appear in a table of the spec but are not forms of their own
IN_TABLES_NOT_FORMS = {
    "ok", "err", "some", "none", "pub", "read", "else", "spawn_with", "res",
}


# words in the grammar that are not the head of a form: keywords of a definition or of a clause, and the types
NOT_HEADS = {
    "module", "import", "as", "language", "pub", "read", "struct", "value", "enum", "const", "union", "test", "fn", "where",
    "plain", "interface", "impl", "req", "ens", "inv", "unsafe", "no_leak", "result", "ptr", "arr", "ok", "err", "else", "->",
    "i32", "i64", "f32", "f64", "bool", "str", "void", "u8", "i8", "u16", "i16", "(", ")", "[", "]", ":", "-", "+", "*", "/", "%", "^",
}
# what the Rust parser lists as a head and the language does not have as source: the lowered forms, and the refused ones
NOT_SOURCE = {"format.assert", "format.dbg", "format.eprintln", "format.expect", "format.piece", "format.print",
              "format.println", "format.str", "format.to", "mem.free", "mem.load_f32", "mem.load_f64", "mem.store_f32", "mem.store_f64"}


def heads_of_parser():
    """The heads src/parser.rs lists (HEAD_NAMES), while the Rust parser is in the repository: the grammar and it must
    name the same forms. Skipped without it."""
    path = os.path.join(ROOT, "src", "parser.rs")
    if not os.path.exists(path):
        return None
    text = open(path, encoding="utf-8").read()
    m = re.search(r"pub const HEAD_NAMES: &\[&str\] = &\[(.*?)\];", text, re.S)
    return set(re.findall(r'"([^"]+)"', m.group(1))) if m else None


def main():
    verbose = "--verbose" in sys.argv
    files = [a for a in sys.argv[1:] if not a.startswith("--")]
    findings = []
    try:
        g = Grammar(spec_ebnf())
    except ValueError as e:
        print(e)
        return 1
    rec = Recogniser(g)

    for src, why in NEGATIVE:
        if rec.recognise(tokenize(src), "program"):
            findings.append(f"the grammar accepts what it must refuse ({why}): {src}")

    for rel in files or program_files():
        if rel in NOT_PROGRAMS:
            continue
        path = rel if os.path.isabs(rel) else os.path.join(ROOT, rel)
        text = open(path, encoding="utf-8").read()
        if re.search(r"^;;\s*@syntax\s*$", text, re.M):
            # a conformance case that is refused for its syntax: the grammar must refuse it too
            try:
                accepted = rec.recognise(tokenize(text), "program")
            except ValueError:
                accepted = False
            if accepted:
                findings.append(f"{rel}: is marked @syntax (a syntax error), and the grammar accepts it")
            continue
        check(rec, rel, text, findings, verbose)
    if not files:
        for label, code in doc_modules():
            check(rec, label, code, findings, verbose)
        for head in forms_in_spec(g):
            findings.append(f"AIPL_SPEC.md names the form ({head} ...) in a table, and the grammar has no such terminal")

    heads = heads_of_parser()
    if heads is not None and not files:
        terminals = g.terminals()
        for h in sorted(heads - terminals - NOT_SOURCE):
            findings.append(f"src/parser.rs reads the head ({h} ...) and the grammar has no such terminal")
        for t in sorted(terminals - heads - NOT_HEADS):
            findings.append(f"the grammar has the terminal \"{t}\", which src/parser.rs does not read as a head")

    for f in findings[:100]:
        print(f)
    print(f"{len(findings)} findings")
    return min(len(findings), 100)


if __name__ == "__main__":
    sys.setrecursionlimit(1_000_000)
    threading.stack_size(512 * 1024 * 1024)
    result = []
    t = threading.Thread(target=lambda: result.append(main()))
    t.start()
    t.join()
    sys.exit(result[0] if result else 1)
