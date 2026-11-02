#!/usr/bin/env python3
"""Every union a value (docs/design/MEMORY_PLAN.md M4 b, decision 6): move
the variants of ast.aipl's Expr and Type that hold sub-expressions or
sub-types, and Expr.make (too wide), behind a pointer to a struct of their
fields, allocated from a named allocator.

  (union Expr [... (if_ at:i32 cond:Expr then_:Expr else_:Expr) ...])
    becomes (if_ n:(ptr IfExpr)), with
  (struct IfExpr [(read at:i32) (read cond:Expr) ...] (pub)) and
  (fn if_expr [a:alloc.Allocator at:i32 cond:Expr ...] -> Expr (pub))

In the files given, a match arm on such a variant binds the struct and
lets its fields under the names the arm bound, and a make of one calls the
constructor with (call alloc.default):

  (ast.Expr.if_ [_ c t f] body...)
    -> (ast.Expr.if_ [n_if] (let c:ast.Expr (get n_if ast.IfExpr.cond)) ... body...)
  (make ast.Expr.if_ at c t f) -> (call ast.if_expr (call alloc.default) at c t f)

Run once: python3 tools/migrate_ast_cells.py aipl_src/ast.aipl aipl_src/*.aipl
(ast.aipl first: its definitions say which fields each variant has). It
prints the struct and constructor definitions for ast.aipl to stdout.
"""
import re
import sys

EXPR_BOXED = ["let_", "set", "if_", "loop", "while_", "ok", "err", "match_result", "get_", "put_",
              "arr_new", "arr_get", "arr_set", "arr_len", "cast", "make_ptr", "make", "with_",
              "match_", "return_", "call_ref", "addr"]
TYPE_BOXED = ["ptr", "result", "arr", "fn_"]
# the names ast.aipl defines, qualified as ast.X elsewhere
AST_NAMES = {"Expr", "Type", "MaybeType", "MaybeExpr", "Step", "Arm", "Op", "AddrKind", "Contract"}


def camel(v):
    return "".join(p.capitalize() for p in v.strip("_").split("_"))


def struct_name(union, v):
    return camel(v) + union


def ctor_name(union, v):
    return v.rstrip("_") + "_" + union.lower()


def binder_name(v):
    return "n_" + v.rstrip("_")


# ---- s-expressions with source positions ----

class Node:
    def __init__(self, kind, start, end, text=None):
        self.kind, self.start, self.end, self.text, self.kids = kind, start, end, text, []


def parse(src):
    stack = [Node("root", 0, len(src))]
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == ";":
            while i < n and src[i] != "\n":
                i += 1
        elif c in "([":
            node = Node("list" if c == "(" else "vec", i, -1)
            stack[-1].kids.append(node)
            stack.append(node)
            i += 1
        elif c in ")]":
            node = stack.pop()
            node.end = i + 1
            i += 1
        elif c == '"':
            j = i + 1
            while src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            stack[-1].kids.append(Node("atom", i, j + 1, src[i:j + 1]))
            i = j + 1
        elif c.isspace():
            i += 1
        else:
            j = i
            # an atom ends at whitespace or a bracket; a type like x:(ptr T) is
            # an atom x: followed by a list
            while j < n and not src[j].isspace() and src[j] not in "()[];\"":
                j += 1
            stack[-1].kids.append(Node("atom", i, j, src[i:j]))
            i = j
    assert len(stack) == 1, "unbalanced"
    return stack[0]


# ---- ast.aipl's variant fields ----

def variant_fields(src, union):
    """{variant: [(field, type text)]} of union `union` in ast.aipl."""
    def find(node):
        for c in node.kids:
            if c.kind == "list" and len(c.kids) > 2 and c.kids[0].text == "union" and c.kids[1].text == union:
                return c
            if c.kind == "list":
                r = find(c)
                if r:
                    return r
        return None
    vec = find(parse(src)).kids[2]
    out = {}
    for v in vec.kids:
        if v.kind != "list":
            continue
        name = v.kids[0].text
        fields = []
        # each field is an atom "name:" then its type: the rest of the atom,
        # or the list that follows it
        ks = v.kids[1:]
        i = 0
        while i < len(ks):
            f, _, t = ks[i].text.partition(":")
            if t == "":
                t = src[ks[i + 1].start:ks[i + 1].end]
                i += 1
            fields.append((f, t))
            i += 1
        out[name] = fields
    return out


def qualify(ty):
    return re.sub(r"\b(" + "|".join(sorted(AST_NAMES)) + r")\b(?!\.)", r"ast.\1", ty)


# ---- rewriting a file ----

def rewrite(src, fields, in_ast):
    pre = "" if in_ast else "ast."
    head_re = re.compile(r"^" + re.escape(pre) + r"(Expr|Type)\.(\w+)$")
    out = []

    def boxed(union, v):
        return v in (EXPR_BOXED if union == "Expr" else TYPE_BOXED)

    def emit(node, active):
        """node's text, rewritten."""
        if node.kind == "atom":
            return node.text
        k = node.kids
        # a match arm on a boxed variant
        if node.kind == "list" and len(k) >= 2 and k[0].kind == "atom" and k[1].kind == "vec":
            m = head_re.match(k[0].text)
            if m and boxed(m.group(1), m.group(2)):
                union, v = m.group(1), m.group(2)
                fs = fields[union][v]
                binders = [b.text for b in k[1].kids]
                assert len(binders) == len(fs), (v, binders)
                if all(b == "_" for b in binders):
                    head = "[_]"
                    lets = ""
                    inner_active = active
                else:
                    nb = binder_name(v)
                    d = 2
                    while nb in active:
                        nb = binder_name(v) + str(d)
                        d += 1
                    inner_active = active | {nb}
                    sn = pre + struct_name(union, v)
                    lets = "".join(
                        f" (let {b}:{qualify(t) if not in_ast else t} (get {nb} {sn}.{f}))"
                        for b, (f, t) in zip(binders, fs) if b != "_")
                    head = f"[{nb}]"
                return (src[node.start:k[1].start] + head + lets
                        + emit_span(k[1].end, node.end, k[2:], inner_active))
        # a make of a boxed variant
        if node.kind == "list" and len(k) >= 2 and k[0].kind == "atom" and k[0].text == "make" and k[1].kind == "atom":
            m = head_re.match(k[1].text)
            if m and boxed(m.group(1), m.group(2)):
                union, v = m.group(1), m.group(2)
                return (src[node.start:k[0].start] + f"call {pre}{ctor_name(union, v)} (call alloc.default)"
                        + emit_span(k[1].end, node.end, k[2:], active))
        return emit_span(node.start, node.end, k, active)

    def emit_span(start, end, kids, active):
        """The text from start to end, the kids in it rewritten."""
        s, at = [], start
        for c in kids:
            s.append(src[at:c.start])
            s.append(emit(c, active))
            at = c.end
        s.append(src[at:end])
        return "".join(s)

    root = parse(src)
    return emit_span(0, len(src), root.kids, frozenset())


def definitions(fields):
    """The structs and constructors ast.aipl gains."""
    out = []
    for union, boxed in (("Type", TYPE_BOXED), ("Expr", EXPR_BOXED)):
        for v in boxed:
            fs = fields[union][v]
            sn = struct_name(union, v)
            out.append(f"  (struct {sn} [" + " ".join(f"(read {f}:{t})" for f, t in fs) + "] (pub))")
    out.append("")
    for union, boxed in (("Type", TYPE_BOXED), ("Expr", EXPR_BOXED)):
        for v in boxed:
            fs = fields[union][v]
            sn = struct_name(union, v)
            params = " ".join(f"{f}:{t}" for f, t in fs)
            out.append(f"  (fn {ctor_name(union, v)} [a:alloc.Allocator {params}] -> {union} (pub)")
            out.append(f"    (let n:(ptr {sn}) (call (alloc.create {sn}) a))")
            for f, _ in fs:
                out.append(f"    (put n {sn}.{f} {f})")
            out.append(f"    (make {union}.{v} n))")
    return "\n".join(out)


def main():
    files = sys.argv[1:]
    ast_src = open(files[0]).read()
    fields = {u: variant_fields(ast_src, u) for u in ("Expr", "Type")}
    for p in dict.fromkeys(files):
        src = open(p).read()
        new = rewrite(src, fields, p.split("/")[-1] == "ast.aipl")
        if new != src:
            open(p, "w").write(new)
            print(f"rewrote {p}", file=sys.stderr)
    print(definitions(fields))


if __name__ == "__main__":
    main()
