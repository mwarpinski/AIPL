#!/usr/bin/env python3
"""Fuzzes the three ways of running a program against each other.

Each case is a generated program: well-typed by construction, so it passes
the checker, and terminating by construction (loops have small constant
bounds, while loops a counter, and functions call only functions defined
before them). It prints what it computes as it goes. The program runs in
the VM (`aipl eval`), compiled to wasm under `aipl-run`, and as a native
executable (`aipl compile --exe`). A case fails when any of them crashes or
hangs, when the checker or compiler rejects it (a generator bug, or a
checker bug), or when they disagree: different output, or one fails where
another does not, or they fail differently. aipl-run and the native
executable must agree byte for byte; the VM words its errors its own way,
so its failure is compared by kind (division by zero, bounds, ...).

    cargo build --release
    python3 tools/run_fuzz.py                  # 300 cases, seed 1
    python3 tools/run_fuzz.py --cases 5000 --seed 7

Failing cases are kept under target/run_fuzz/ (prog.aipl beside a note.txt
saying what went wrong); passing ones are deleted. The exit status is the
number of failures (capped at 100).
"""

import argparse
import concurrent.futures
import os
import random
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release")
AIPL = os.path.join(BIN, "aipl")
RUNNER = os.path.join(BIN, "aipl-run")
OUT = os.path.join(ROOT, "target", "run_fuzz")
TIMEOUT = 30

INT_OPS = ["+", "-", "*", "^", "bitand", "bitor", "shl", "shr", "shru"]
DIV_OPS = ["/", "%", "divu", "remu"]
CHECKED = ["checked.add", "checked.sub", "checked.mul"]
CMP = ["eq", "neq", "lt", "lte", "gt", "gte"]
UCMP = ["ltu", "lteu", "gtu", "gteu"]
EDGE32 = ["0", "1", "-1", "2", "7", "31", "32", "100", "-100", "2147483647", "-2147483648", "65536", "4294967295"]
EDGE64 = ["0i64", "1i64", "-1i64", "3i64", "63i64", "64i64", "1000i64", "9223372036854775807i64",
          "-9223372036854775808i64", "4294967296i64", "-4294967296i64"]
FLOATS = ["0.0", "-0.0", "1.0", "-1.0", "0.5", "0.1", "2.5", "3.141592653589793", "1.0e10", "1.5e-7",
          "-123.456", "1.0e300", "4.9e-324", "1.7976931348623157e308"]
SCALARS = ["i32", "i64", "f64", "bool"]
R = "(result i32 bool)"
F = "(fn [i32] -> i32)"
STRINGS = ['""', '"a"', '"ab"', '"hello"', '"\\n\\t"']


class Gen:
    def __init__(self, rng):
        self.rng = rng
        self.n = 0
        nstructs = rng.randint(1, 3)
        self.structs = {}
        for k in range(nstructs):
            fields = [(f"f{j}", rng.choice(SCALARS + ["E"])) for j in range(rng.randint(1, 4))]
            self.structs[f"S{k}"] = fields
        self.enum = ["a", "b", ("c", rng.choice([2, 5, 10])), "d"]
        self.variants = [("p", [("x", "i32"), ("y", "i64")]), ("q", [("z", "f64")]), ("r", [])]
        # (name, [(param, type)], ret, recursive); a recursive function's
        # first parameter counts down to 0, from at most 3
        self.fns = []

    # ----- names and types -----

    def fresh(self, base="v"):
        self.n += 1
        return f"{base}{self.n}"

    def any_type(self):
        r = self.rng.random()
        if r < 0.55:
            return self.rng.choice(SCALARS)
        if r < 0.65:
            return "E"
        if r < 0.75:
            return "U"
        if r < 0.84:
            return f"(ptr {self.rng.choice(list(self.structs))})"
        if r < 0.92:
            return f"(arr {self.rng.choice(SCALARS + ['E'])})"
        if r < 0.96 or not self.targets():
            return R
        return F

    def targets(self):
        """The functions a reference may name: (fn [i32] -> i32) ones. They
        have no reference parameters, so a reference only ever calls into
        functions defined before the one that made it, and calls end. Not
        recursive ones: a reference passes any count, not one below 4."""
        return [f[0] for f in self.fns if f[2] == "i32" and [t for _, t in f[1]] == ["i32"] and not f[3]]

    def enum_members(self):
        return [m if isinstance(m, str) else m[0] for m in self.enum]

    def enum_values(self):
        out, v = [], 0
        for m in self.enum:
            if isinstance(m, tuple):
                v = m[1]
            out.append(v)
            v += 1
        return out

    # ----- expressions -----

    def var_of(self, ctx, t):
        names = [n for n, (vt, _) in ctx.vars.items() if vt == t]
        return self.rng.choice(names) if names else None

    def literal(self, t):
        rng = self.rng
        if t == "i32":
            return rng.choice(EDGE32) if rng.random() < 0.4 else str(rng.randint(-50, 50))
        if t == "i64":
            return rng.choice(EDGE64) if rng.random() < 0.4 else f"{rng.randint(-50, 50)}i64"
        if t == "f64":
            return rng.choice(FLOATS)
        if t == "bool":
            return rng.choice(["true", "false"])
        if t == "E":
            return "E." + rng.choice(self.enum_members())
        if t == "U":
            return self.make_union(None, 0)
        if t.startswith("(ptr "):
            return f"(new {t[5:-1]})"
        if t.startswith("(arr "):
            el = t[5:-1]
            return f"(arr.new {el} {rng.randint(1, 5)})"
        if t == R:
            return f"(ok:bool {self.literal('i32')})" if rng.random() < 0.5 else f"(err {self.literal('bool')})"
        if t == F:
            return f"(ref {rng.choice(self.targets())})"
        raise ValueError(t)

    def expr(self, ctx, t, d):
        rng = self.rng
        if d <= 0 or rng.random() < 0.25:
            v = self.var_of(ctx, t)
            if v and rng.random() < 0.7:
                return v
            return self.literal(t)
        choices = {
            "i32": [self.int_op, self.int_op, self.div_op, self.checked_op, self.conv_to_i32, self.struct_get,
                    self.arr_get, self.arr_len, self.call, self.if_expr, self.block_expr, self.match_expr,
                    self.enum_ord, self.cond_expr, self.match_result, self.misc_i32, self.call_ref],
            "i64": [self.int_op, self.int_op, self.div_op, self.checked_op, self.conv_to_i64, self.struct_get,
                    self.arr_get, self.call, self.if_expr, self.match_expr, self.cond_expr, self.match_result],
            "f64": [self.float_op, self.float_op, self.conv_to_f64, self.struct_get, self.arr_get, self.call,
                    self.if_expr, self.match_expr, self.cond_expr],
            "bool": [self.compare, self.compare, self.logic, self.struct_get, self.arr_get, self.call, self.if_expr,
                     self.misc_bool, self.match_result],
            "E": [self.enum_cast, self.struct_get, self.arr_get, self.call, self.if_expr, self.match_expr],
            "U": [self.make_union, self.call, self.if_expr],
            R: [self.make_result, self.call, self.if_expr],
        }.get(t)
        if choices is None:  # pointers and arrays
            choices = [self.call, self.if_expr]
        for _ in range(4):
            e = rng.choice(choices)(ctx, t, d - 1)
            if e is not None:
                return e
        return self.literal(t)

    def cond_expr(self, ctx, t, d):
        clauses = " ".join(f"({self.expr(ctx, 'bool', d)} {self.expr(ctx, t, d)})" for _ in range(self.rng.randint(1, 3)))
        return f"(cond {clauses} (else {self.expr(ctx, t, d)}))"

    def make_result(self, ctx, t, d):
        if self.rng.random() < 0.5:
            return f"(ok:bool {self.expr(ctx, 'i32', d)})"
        return f"(err {self.expr(ctx, 'bool', d)})"

    def match_result(self, ctx, t, d):
        ok_arm, err_arm = ctx.child(), ctx.child()
        x, y = self.fresh("k"), self.fresh("e")
        ok_arm.vars[x] = ("i32", False)
        err_arm.vars[y] = ("bool", False)
        return (f"(match_result {self.expr(ctx, R, d)} (ok {x} {self.expr(ok_arm, t, d)}) "
                f"(err {y} {self.expr(err_arm, t, d)}))")

    def misc_i32(self, ctx, t, d):
        r = self.rng.random()
        if r < 0.4:
            return f"(str.len {self.rng.choice(STRINGS)})"
        what = self.rng.choice(list(self.structs) + SCALARS + ["E", "(ptr S0)", "(arr i64)"])
        return f"(sizeof {what})"

    def misc_bool(self, ctx, t, d):
        if self.rng.random() < 0.6 or not self.targets():
            return f"({self.rng.choice(['eq', 'neq'])} {self.rng.choice(STRINGS)} {self.rng.choice(STRINGS)})"
        return f"({self.rng.choice(['eq', 'neq'])} {self.expr(ctx, F, d)} {self.expr(ctx, F, d)})"

    def call_ref(self, ctx, t, d):
        if not self.targets():
            return None
        return f"(call_ref {F} {self.expr(ctx, F, d)} {self.expr(ctx, 'i32', d)})"

    def int_op(self, ctx, t, d):
        op = self.rng.choice(INT_OPS)
        return f"({op} {self.expr(ctx, t, d)} {self.expr(ctx, t, d)})"

    def div_op(self, ctx, t, d):
        op = self.rng.choice(DIV_OPS)
        divisor = self.expr(ctx, t, d)
        if self.rng.random() < 0.85:
            one = "1" if t == "i32" else "1i64"
            divisor = f"(bitor {divisor} {one})"
        return f"({op} {self.expr(ctx, t, d)} {divisor})"

    def checked_op(self, ctx, t, d):
        op = self.rng.choice(CHECKED)
        a, b = self.expr(ctx, t, d), self.expr(ctx, t, d)
        if self.rng.random() < 0.7:  # mostly small enough not to overflow
            mask = "1023" if t == "i32" else "1023i64"
            a, b = f"(bitand {a} {mask})", f"(bitand {b} {mask})"
        return f"({op} {a} {b})"

    def conv_to_i32(self, ctx, t, d):
        return f"(i32.wrap {self.expr(ctx, 'i64', d)})"

    def conv_to_i64(self, ctx, t, d):
        r = self.rng.random()
        if r < 0.35:
            return f"(i64.extend_s {self.expr(ctx, 'i32', d)})"
        if r < 0.6:
            return f"(i64.extend_u {self.expr(ctx, 'i32', d)})"
        if r < 0.75:
            return f"(i64.reinterpret_f64 {self.expr(ctx, 'f64', d)})"
        x = self.expr(ctx, "f64", d)
        if self.rng.random() < 0.85:  # in range (NaN fails both tests), so it does not trap
            t = self.fresh("t")
            x = f"(block (let {t}:f64 {x}) (if (and (lt {t} 1.0e15) (gt {t} -1.0e15)) {t} 0.5))"
        return f"(i64.trunc_f64_s {x})"

    def conv_to_f64(self, ctx, t, d):
        r = self.rng.random()
        if r < 0.5:
            return f"(f64.convert_i64_s {self.expr(ctx, 'i64', d)})"
        if r < 0.8:
            return f"(f64.sqrt {self.expr(ctx, 'f64', d)})"
        return f"(f64.reinterpret_i64 {self.expr(ctx, 'i64', d)})"

    def float_op(self, ctx, t, d):
        op = self.rng.choice(["+", "-", "*", "/"])
        return f"({op} {self.expr(ctx, 'f64', d)} {self.expr(ctx, 'f64', d)})"

    def compare(self, ctx, t, d):
        rng = self.rng
        at = rng.choice(["i32", "i32", "i64", "f64", "E", "ptr"])
        if at == "E":
            return f"({rng.choice(['eq', 'neq'])} {self.expr(ctx, 'E', d)} {self.expr(ctx, 'E', d)})"
        if at == "ptr":
            s = rng.choice(list(self.structs))
            a = self.var_of(ctx, f"(ptr {s})")
            if a is None:
                return None
            b = self.var_of(ctx, f"(ptr {s})") or f"(ptr.null {s})"
            return f"({rng.choice(['eq', 'neq'])} {a} {b})"
        ops = CMP + (UCMP if at != "f64" else [])
        return f"({rng.choice(ops)} {self.expr(ctx, at, d)} {self.expr(ctx, at, d)})"

    def logic(self, ctx, t, d):
        r = self.rng.random()
        if r < 0.3:
            return f"(not {self.expr(ctx, 'bool', d)})"
        op = "and" if r < 0.65 else "or"
        return f"({op} {self.expr(ctx, 'bool', d)} {self.expr(ctx, 'bool', d)})"

    def enum_ord(self, ctx, t, d):
        return f"(enum.ord {self.expr(ctx, 'E', d)})"

    def enum_cast(self, ctx, t, d):
        return f"(enum.cast E {self.rng.choice(self.enum_values())})"

    def struct_get(self, ctx, t, d):
        options = [(s, f) for s, fs in self.structs.items() for f, ft in fs if ft == t]
        self.rng.shuffle(options)
        for s, f in options:
            p = self.var_of(ctx, f"(ptr {s})")
            if p:
                return f"(get {p} {s}.{f})"
        return None

    def index(self, ctx, a, d):
        i = self.expr(ctx, "i32", d)
        if self.rng.random() < 0.9:
            return f"(remu {i} (arr.len {a}))"
        return i

    def arr_get(self, ctx, t, d):
        a = self.var_of(ctx, f"(arr {t})")
        if a is None:
            return None
        return f"(arr.get {t} {a} {self.index(ctx, a, d)})"

    def arr_len(self, ctx, t, d):
        arrs = [n for n, (vt, _) in ctx.vars.items() if vt.startswith("(arr ")]
        return f"(arr.len {self.rng.choice(arrs)})" if arrs else None

    def call(self, ctx, t, d):
        fs = [f for f in self.fns if f[2] == t]
        me = ctx.fn
        if me is not None and me[3] and me[2] == t and self.rng.random() < 0.3:
            fs = [me]
        if not fs:
            return None
        return self.call_of(ctx, self.rng.choice(fs), min(d, 1))

    def call_of(self, ctx, f, d):
        name, params, _, recursive = f
        args = [self.expr(ctx, pt, d) for _, pt in params]
        if recursive:
            n = params[0][0]
            args[0] = f"(- {n} 1)" if ctx.fn is f else f"(bitand {args[0]} 3)"
        return f"(call {name}{''.join(' ' + a for a in args)})"

    def if_expr(self, ctx, t, d):
        return f"(if {self.expr(ctx, 'bool', d)} {self.expr(ctx, t, d)} {self.expr(ctx, t, d)})"

    def block_expr(self, ctx, t, d):
        inner = ctx.child()
        stmts = [self.stmt(inner, d) for _ in range(self.rng.randint(1, 2))]
        return f"(block {' '.join(stmts)} {self.expr(inner, t, d)})"

    def make_union(self, ctx, t, d=0):
        name, fields = self.rng.choice(self.variants)
        args = " ".join(self.literal(ft) if ctx is None else self.expr(ctx, ft, d) for _, ft in fields)
        return f"(make U.{name}{' ' + args if args else ''})"

    def match_expr(self, ctx, t, d):
        rng = self.rng
        if rng.random() < 0.5:
            v = self.expr(ctx, "E", d)
            members = self.enum_members()
            arms = members if rng.random() < 0.5 else rng.sample(members, 2)
            out = [f"(E.{m} {self.expr(ctx, t, d)})" for m in arms]
            if len(arms) < len(members):
                out.append(f"(else {self.expr(ctx, t, d)})")
            return f"(match {v} {' '.join(out)})"
        v = self.expr(ctx, "U", d)
        out = []
        for name, fields in self.variants:
            arm = ctx.child()
            binders = []
            for _, ft in fields:
                if rng.random() < 0.2:
                    binders.append("_")
                else:
                    b = self.fresh("m")
                    arm.vars[b] = (ft, False)
                    binders.append(b)
            bind = f" [{' '.join(binders)}]" if fields else ""
            out.append(f"(U.{name}{bind} {self.expr(arm, t, d)})")
        return f"(match {v} {' '.join(out)})"

    # ----- statements -----

    def stmt(self, ctx, d):
        rng = self.rng
        r = rng.random()
        if r < 0.22 or not ctx.vars:
            t = self.any_type()
            name = self.fresh()
            e = self.expr(ctx, t, d + 1)
            ctx.vars[name] = (t, True)
            return f"(let {name}:{t} {e})"
        if r < 0.36:
            mutable = [n for n, (_, m) in ctx.vars.items() if m]
            if mutable:
                n = rng.choice(mutable)
                return f"(set! {n} {self.expr(ctx, ctx.vars[n][0], d + 1)})"
        if r < 0.5:
            return self.print_stmt(ctx, self.rng.choice(SCALARS + ["E"]), d)
        if d <= 0:
            return self.print_stmt(ctx, "i32", 0)
        if r < 0.58:
            return f"(if {self.expr(ctx, 'bool', d)} {self.body(ctx, d - 1)} {self.body(ctx, d - 1)})"
        if r < 0.66:
            i = self.fresh("i")
            inner = ctx.child(loop=True)
            inner.vars[i] = ("i32", False)
            lo = rng.randint(-2, 3)
            hi = lo + rng.randint(-1, 4)
            return f"(loop {i} {lo} {hi} {rng.randint(1, 2)} {self.stmts(inner, d - 1)})"
        if r < 0.72:
            c = self.fresh("w")
            inner = ctx.child(loop=True)
            cond = self.expr(ctx, "bool", d - 1)
            return (f"(let {c}:i32 0) (while (and (lt {c} {rng.randint(1, 5)}) {cond}) "
                    f"(set! {c} (+ {c} 1)) {self.stmts(inner, d - 1)})")
        if r < 0.78 and ctx.loop:
            return f"(if {self.expr(ctx, 'bool', d - 1)} ({rng.choice(['break', 'continue'])}) (block))"
        if r < 0.84:
            ptrs = [(n, vt[5:-1]) for n, (vt, _) in ctx.vars.items() if vt.startswith("(ptr ")]
            if ptrs:
                p, s = rng.choice(ptrs)
                f, ft = rng.choice(self.structs[s])
                return f"(put {p} {s}.{f} {self.expr(ctx, ft, d)})"
        if r < 0.9:
            arrs = [(n, vt[5:-1]) for n, (vt, _) in ctx.vars.items() if vt.startswith("(arr ")]
            if arrs:
                a, el = rng.choice(arrs)
                return f"(arr.set {el} {a} {self.index(ctx, a, d)} {self.expr(ctx, el, d)})"
        if r < 0.92:
            return self.memory_stmt(ctx, d)
        if r < 0.94 and ctx.ret is not None:
            v = "" if ctx.ret == "void" else " " + self.expr(ctx, ctx.ret, d)
            return f"(if {self.expr(ctx, 'bool', d - 1)} (return{v}) (block))"
        return self.print_stmt(ctx, rng.choice(SCALARS), d)

    def memory_stmt(self, ctx, d):
        m = self.fresh("mem")
        return (f"(let {m}:i32 (mem.alloc 24)) (mem.store32 {m} {self.expr(ctx, 'i32', d)}) "
                f"(mem.store64 (+ {m} 8) {self.expr(ctx, 'i64', d)}) (mem.store8 (+ {m} 17) {self.expr(ctx, 'i32', d)}) "
                + " ".join(self.print_value(t, e) for t, e in [
                    ("i32", f"(mem.load32 {m})"), ("i64", f"(mem.load64 (+ {m} 8))"), ("i32", f"(mem.load8 (+ {m} 17))"),
                    ("i32", f"(mem.load32 (+ {m} 16))"), ("i32", f"(mem.load8 (+ {m} 3))")]))

    def stmts(self, ctx, d):
        return " ".join(self.stmt(ctx, d) for _ in range(self.rng.randint(1, 3)))

    def body(self, ctx, d):
        return f"(block {self.stmts(ctx.child(), d)})"

    def print_stmt(self, ctx, t, d):
        return self.print_value(t, self.expr(ctx, t, d))

    def show(self, ctx, t, e):
        """Statements printing everything in the value of e, of type t."""
        if t in SCALARS or t == "E":
            return self.print_value(t, e)
        v = self.fresh("s")
        out = [f"(let {v}:{t} {e})"]
        if t == "U":
            arms = []
            for name, fields in self.variants:
                bs = [self.fresh("m") for _ in fields]
                shown = " ".join(self.print_value(ft, b) for (_, ft), b in zip(fields, bs))
                bind = f" [{' '.join(bs)}]" if fields else ""
                arms.append(f'(U.{name}{bind} (sys.print "{self.fresh("u")} {name}") {shown})')
            out.append(f"(match {v} {' '.join(arms)})")
        elif t.startswith("(ptr "):
            s = t[5:-1]
            out.append(self.print_value("i32", f"(ptr.addr {v})"))  # the heap is laid out alike everywhere
            out += [self.print_value(ft, f"(get {v} {s}.{f})") for f, ft in self.structs[s]]
        elif t == R:
            x, y = self.fresh("k"), self.fresh("e")
            out.append(f"(match_result {v} (ok {x} {self.print_value('i32', x)}) (err {y} {self.print_value('bool', y)}))")
        elif t == F:
            out.append(self.print_value("i32", f"(call_ref {F} {v} {self.literal('i32')})"))
        else:
            el = t[5:-1]
            i = self.fresh("i")
            out.append(self.print_value("i32", f"(arr.addr {v})"))
            out.append(self.print_value("i32", f"(arr.len {v})"))
            out.append(f"(loop {i} 0 (- (arr.len {v}) 1) 1 {self.print_value(el, f'(arr.get {el} {v} {i})')})")
        return " ".join(out)

    def print_value(self, t, e):
        label = self.fresh("p")
        if t == "i32":
            return f'(call io.println_int "{label} " {e})'
        if t == "i64":
            return f'(call io.println_i64 "{label} " {e})'
        if t == "f64":
            return f'(call io.println_f64 "{label} " {e} {self.rng.choice([0, 3, 17])})'
        if t == "E":
            return f'(call io.println_int "{label} " (enum.ord {e}))'
        return f'(if {e} (sys.print "{label} T") (sys.print "{label} F"))'

    # ----- the program -----

    def function(self, k):
        rng = self.rng
        name = f"g{k}"
        if k == 0 or rng.random() < 0.25:  # a reference may name it
            params, ret = [(self.fresh("a"), "i32")], "i32"
        else:
            params = [(self.fresh("a"), self.any_type()) for _ in range(rng.randint(0, 3))]
            ret = self.any_type()
        recursive = bool(params) and params[0][1] == "i32" and rng.random() < 0.3
        f = (name, params, ret, recursive)
        ctx = Ctx(ret=ret)
        ctx.fn = f
        for p, t in params:
            ctx.vars[p] = (t, False)
        contracts = ""
        ints = [p for p, t in params if t == "i32"]
        if ints and rng.random() < 0.3:
            contracts = f" (req (gt {rng.choice(ints)} {rng.randint(-40, 0)}))"
        if ret == "i32" and rng.random() < 0.2:
            contracts += f" (ens (neq res {self.literal('i32')}))"
        body = ""
        if recursive:
            body = f"(if (lte {params[0][0]} 0) (return {self.literal(ret)}) (block)) "
        body += self.stmts(ctx, 2)
        # what the body computed, before the result
        body += " " + " ".join(self.show(ctx, t, n) for n, (t, _) in list(ctx.vars.items()) if self.rng.random() < 0.5)
        result = self.expr(ctx, ret, 3)
        ps = " ".join(f"{p}:{t}" for p, t in params)
        self.fns.append(f)
        return f"  (fn {name} [{ps}] -> {ret}{contracts}\n    {body}\n    {result})"

    def program(self):
        rng = self.rng
        lines = ["(module prog", "  (import io)"]
        for s, fs in self.structs.items():
            lines.append(f"  (struct {s} [{' '.join(f'{f}:{t}' for f, t in fs)}])")
        members = " ".join(m if isinstance(m, str) else f"({m[0]} {m[1]})" for m in self.enum)
        lines.append(f"  (enum E [{members}])")
        vs = " ".join(f"({n}{''.join(' ' + f + ':' + t for f, t in fs)})" for n, fs in self.variants)
        lines.append(f"  (union U [{vs}])")
        for k in range(rng.randint(1, 5)):
            lines.append(self.function(k))
        ctx = Ctx(ret=None)  # no early return: the summary below always runs
        body = [self.stmt(ctx, 3) for _ in range(rng.randint(4, 10))]
        # every function's result, then every variable of main
        for f in self.fns:
            body.append(self.show(ctx, f[2], self.call_of(ctx, f, 2)))
        body += [self.show(ctx, t, n) for n, (t, _) in list(ctx.vars.items())]
        lines.append(f"  (fn main [] -> i32\n    {chr(10).join('    ' + b for b in body).lstrip()}\n    0))")
        return "\n".join(lines) + "\n"


class Ctx:
    def __init__(self, ret=None, loop=False, parent=None):
        self.vars = dict(parent.vars) if parent else {}
        self.ret = ret if parent is None else parent.ret
        self.fn = parent.fn if parent else None
        self.loop = loop or (parent.loop if parent else False)

    def child(self, loop=False):
        return Ctx(loop=loop, parent=self)


def run(cmd, cwd):
    """(exit status, stdout, stderr); status None on a timeout."""
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT, cwd=cwd)
        return r.returncode, r.stdout.decode("utf8", "replace"), r.stderr.decode("utf8", "replace")
    except subprocess.TimeoutExpired:
        return None, "", "timeout"


def vm_failure(err):
    """The VM's error message, or None."""
    m = re.search(r"^Error: (.*)", err, re.M | re.S)
    return m.group(1).strip() if m else None


def trap_message(err, prog):
    """A compiled program's trap message without the program name or call chain."""
    first = err.split("\n", 1)[0]
    return first[len(prog) + 2:] if first.startswith(prog + ": ") else first


def kind(msg):
    """A failure's kind, so the VM's wording and the hosts' can be compared."""
    m = msg.lower()
    if m.startswith("wasm trap: "):
        m = m[len("wasm trap: "):]
    if "divide by zero" in m or "division by zero" in m:
        return "divide by zero"
    if "overflow" in m:
        return "integer overflow"
    if "invalid conversion" in m or "trunc" in m:
        return "bad conversion"
    if m.startswith("array index out of bounds"):
        return msg  # the same text everywhere
    if "condition failed in" in m:
        # the VM adds the position: "in 'f' at 3:10: (req" -> "in 'f': (req"
        return re.sub(r"' at [0-9]+:[0-9]+: ", "': ", msg)
    if "unreachable" in m:
        return "unreachable"
    return msg


def same_failure(vm, compiled):
    """Whether the VM's failure and compiled code's are the same one. A
    compiled contract message shows only the values compiled code can print
    (not floats, AIPL_SPEC.md 7.6): each it shows must be the VM's."""
    a, b = kind(vm), kind(compiled)
    if "condition failed in" not in a or " with " not in a:
        return a == b
    head, values = a.split(" with ", 1)
    chead, _, cvalues = b.partition(" with ")
    vm_values = dict(v.split(" = ", 1) for v in values.split(", "))
    shown = [v.split(" = ", 1) for v in cvalues.split(", ")] if cvalues else []
    return head == chead and all(vm_values.get(n) == v for n, v in shown)


def check(case, seed):
    rng = random.Random(seed * 1_000_003 + case)
    src = Gen(rng).program()
    d = os.path.join(OUT, f"{seed}_{case}")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "prog.aipl"), "w") as f:
        f.write(src)

    problems = []
    v, v_out, v_err = run([AIPL, "verify", "prog.aipl"], d)
    if v != 0:
        problems.append(f"rejected (generator or checker bug): {(v_out + v_err).strip()[-400:]}")
    else:
        c, c_out, c_err = run([AIPL, "compile", "prog.aipl", "-o", "prog.wasm"], d)
        n, n_out, n_err = run([AIPL, "compile", "--exe", "prog.aipl", "-o", "prog"], d)
        if c != 0:
            problems.append(f"verify accepts, compile fails: {(c_out + c_err).strip()[-400:]}")
        elif n != 0:
            problems.append(f"compile accepts, compile --exe fails: {(n_out + n_err).strip()[-400:]}")
        else:
            outcomes = {}
            e, e_out, e_err = run([AIPL, "eval", "prog.aipl"], d)
            # the VM's own lines: a banner first, the result last
            lines = e_out.split("\n")
            if lines and lines[0].startswith("[AIPL VM]"):
                lines = lines[1:]
            if len(lines) >= 2 and lines[-2].startswith("[AIPL Result]") and lines[-1] == "":
                lines = lines[:-2] + [""]
            outcomes["vm"] = (e, "\n".join(lines), vm_failure(e_err) if e != 0 else None, e_err)
            w, w_out, w_err = run([RUNNER, "prog.wasm"], d)
            outcomes["wasm"] = (w, w_out, trap_message(w_err, "prog.wasm") if w != 0 else None, w_err)
            x, x_out, x_err = run(["./prog"], d)
            outcomes["native"] = (x, x_out, trap_message(x_err, "./prog") if x != 0 else None, x_err)
            for name, (status, out, fail, err) in outcomes.items():
                if status is None:
                    problems.append(f"{name} hangs")
                elif "panicked" in err or status not in (0, 1, 134):
                    problems.append(f"{name} crashed (exit {status}): {err[-300:]}")
            if not problems:
                vm, wasm, native = outcomes["vm"], outcomes["wasm"], outcomes["native"]
                if wasm[0] != native[0] or wasm[1] != native[1] or wasm[2] != native[2]:
                    problems.append(f"wasm and native differ: exit {wasm[0]} vs {native[0]}; "
                                    f"failure {wasm[2]!r} vs {native[2]!r}; "
                                    f"output {'same' if wasm[1] == native[1] else 'differs'}")
                elif w_err.split("\n", 1)[1:] != x_err.split("\n", 1)[1:]:
                    problems.append(f"wasm and native call chains differ:\n{w_err}\n---\n{x_err}")
                if (vm[2] is None) != (wasm[2] is None):
                    problems.append(f"the VM {'fails' if vm[2] else 'succeeds'} ({vm[2]}), "
                                    f"compiled code {'fails' if wasm[2] else 'succeeds'} ({wasm[2]})")
                elif vm[2] is not None and not same_failure(vm[2], wasm[2]):
                    problems.append(f"different failures: VM {vm[2]!r}, compiled {wasm[2]!r}")
                if vm[1] != wasm[1]:
                    problems.append("different output: " + first_difference(vm[1], wasm[1]))
    if problems:
        with open(os.path.join(d, "note.txt"), "w") as f:
            f.write("\n".join(problems) + "\n")
    else:
        shutil.rmtree(d)
    return d, problems


def first_difference(a, b):
    la, lb = a.split("\n"), b.split("\n")
    for k, (x, y) in enumerate(zip(la, lb)):
        if x != y:
            return f"line {k + 1}: VM {x!r}, compiled {y!r}"
    return f"VM {len(la)} lines, compiled {len(lb)}"


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--cases", type=int, default=300)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--show", type=int, help="print the program for one case and exit")
    args = ap.parse_args()
    if args.show is not None:
        print(Gen(random.Random(args.seed * 1_000_003 + args.show)).program(), end="")
        return
    for b in (AIPL, RUNNER):
        if not os.path.exists(b):
            sys.exit("build first: cargo build --release")
    os.makedirs(OUT, exist_ok=True)
    failures = 0
    kinds = {}
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as ex:
        for d, problems in ex.map(lambda k: check(k, args.seed), range(args.cases)):
            if problems:
                failures += 1
                k = re.sub(r"[0-9]+", "N", problems[0].split(":")[0].split("(")[0]).strip()
                kinds[k] = kinds.get(k, 0) + 1
                if failures <= 20:
                    print(os.path.relpath(d, ROOT) + ": " + problems[0][:300])
    for k, n in sorted(kinds.items(), key=lambda kv: -kv[1]):
        print(f"  {n:5}  {k}")
    print(f"{args.cases} cases, seed {args.seed}: {failures} failing (kept under target/run_fuzz/)")
    sys.exit(min(failures, 100))


if __name__ == "__main__":
    main()
