#!/usr/bin/env python3
"""Test vectors for aipl_src/native/arm64.aipl (native ARM64 backend task NA2).

Each group below is a list of instruction forms, written once and used twice:
as calls of the encoder (the `test_<group>` functions, between ARM64_TESTS_BEGIN
and ARM64_TESTS_END in arm64.aipl) and as assembly text, which Apple's
assembler (clang -arch arm64, on a Mac, read back with otool) turns into the
expected words. The words are pasted into the tests as hex; the encoder never
produces them. The script rewrites that region of arm64.aipl:

    python3 tools/arm64_vectors.py             # assemble on `ssh pennylane`
    python3 tools/arm64_vectors.py --host HOST # on another Mac
    python3 tools/arm64_vectors.py --local     # with clang on this (arm64 Mac) machine
    python3 tools/arm64_vectors.py --check     # only that the region is up to date

Every loop runs `i` over x0-x30, q(r) = (7r + 3) mod 31 pairs each register with
another; memory forms use the base register (i + 7) mod 31, never the register
they load or store (a writeback form with Rn = Rt is unpredictable).
"""
import os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET = os.path.join(HERE, "..", "aipl_src", "native", "arm64.aipl")
Q = lambda r: (r * 7 + 3) % 31
CC = "eq ne hs lo mi pl vs vc hi ls ge lt gt le".split()

def X(sz, n, zr=True):
    if n == 31:
        return ("xzr" if zr else "sp") if sz == 64 else ("wzr" if zr else "wsp")
    return f"{'x' if sz == 64 else 'w'}{n}"

# an operand: (AIPL text, function of i)
I_ = ("i", lambda i: i)
Q1 = ("(call q i)", lambda i: Q(i))
Q2 = ("(call q (call q i))", lambda i: Q(Q(i)))
Q3 = ("(call q (call q (call q i)))", lambda i: Q(Q(Q(i))))
B_ = ("(% (+ i 7) 31)", lambda i: (i + 7) % 31)       # a base register
N1 = ("(% (+ i 1) 31)", lambda i: (i + 1) % 31)       # a second register, never i
def lit(n): return (str(n), lambda i: n)
def mod(a, b): return (f"(% i {a})" if b == 1 else f"(* (% i {a}) {b})", lambda i: (i % a) * b)
def plus(a, k): return (f"(+ {a[0]} {k})", lambda i: a[1](i) + k)
def nm(n): return ("(% i 14)" if n == 0 else f"(% (+ i {n}) 14)", lambda i: i % 14 if n == 0 else (i + n) % 14)

def rrr(name, sz, a, b, c):
    return lambda v: f"{name} {X(sz,v[0])}, {X(sz,v[1])}, {X(sz,v[2])}"
GROUPS = {}   # name -> [(aipl call text, asm formatter, [operands]) ...] (looped), and fixed ones

def op(fn, args, asm, per="i"):
    """One form: fn is the AIPL function, args the operand list (sz first if it takes one), asm(values) its text"""
    return (fn, args, asm, per)

def mnemonic_forms(names, sizes, ops3):
    out = []
    for n in names:
        m = n[:-4] if n.endswith("_rrr") else n
        for sz, (a, b, c) in sizes:
            out.append(op(n, [lit(sz), a, b, c], lambda v, m=m: f"{m} {X(v[0],v[1])}, {X(v[0],v[2])}, {X(v[0],v[3])}"))
    return out

S3 = [(64, (I_, Q1, Q2)), (32, (Q1, I_, Q2))]
G = {}
G["alu_reg"] = mnemonic_forms(["add_rrr", "sub_rrr", "adds_rrr", "subs_rrr", "and_rrr", "orr_rrr", "eor_rrr", "ands_rrr", "bic_rrr", "orn_rrr", "eon_rrr"], S3, None)
for sz in (64, 32):
    G["alu_reg"] += [
        op("cmp_rr", [lit(sz), I_, Q1], lambda v: f"cmp {X(v[0],v[1])}, {X(v[0],v[2])}"),
        op("cmn_rr", [lit(sz), I_, Q1], lambda v: f"cmn {X(v[0],v[1])}, {X(v[0],v[2])}"),
        op("tst_rr", [lit(sz), I_, Q1], lambda v: f"tst {X(v[0],v[1])}, {X(v[0],v[2])}"),
        op("neg", [lit(sz), I_, Q1], lambda v: f"neg {X(v[0],v[1])}, {X(v[0],v[2])}"),
        op("mvn", [lit(sz), I_, Q1], lambda v: f"mvn {X(v[0],v[1])}, {X(v[0],v[2])}"),
        op("mov_rr", [lit(sz), I_, Q1], lambda v: f"mov {X(v[0],v[1])}, {X(v[0],v[2])}"),
    ]
# zero-register forms, once
G["alu_reg"] += [
    op("add_rrr", [lit(64), lit(5), lit(31), lit(31)], lambda v: "add x5, xzr, xzr", per="once"),
    op("sub_rrr", [lit(32), lit(31), lit(5), lit(6)], lambda v: "sub wzr, w5, w6", per="once"),
    op("mov_rr", [lit(64), lit(31), lit(7)], lambda v: "mov xzr, x7", per="once"),
    op("cmp_rr", [lit(64), lit(31), lit(7)], lambda v: "cmp xzr, x7", per="once"),
]

G["muldiv"] = mnemonic_forms(["udiv", "sdiv", "lslv", "lsrv", "asrv", "rorv", "mul"], S3, None)
for sz in (64, 32):
    G["muldiv"] += [
        op("madd", [lit(sz), I_, Q1, Q2, Q3], lambda v: f"madd {X(v[0],v[1])}, {X(v[0],v[2])}, {X(v[0],v[3])}, {X(v[0],v[4])}"),
        op("msub", [lit(sz), Q1, I_, Q2, Q3], lambda v: f"msub {X(v[0],v[1])}, {X(v[0],v[2])}, {X(v[0],v[3])}, {X(v[0],v[4])}"),
        op("clz", [lit(sz), I_, Q1], lambda v: f"clz {X(v[0],v[1])}, {X(v[0],v[2])}"),
        op("rbit", [lit(sz), Q1, I_], lambda v: f"rbit {X(v[0],v[1])}, {X(v[0],v[2])}"),
    ]
G["muldiv"] = [(f, a, (lambda v, m=m: m(v)), per) for (f, a, m, per) in G["muldiv"]]
# lslv and friends are written lsl, lsr, asr, ror by the assembler
_alias = {"lslv": "lsl", "lsrv": "lsr", "asrv": "asr", "rorv": "ror"}
G["muldiv"] = [(f, a, (lambda v, f=f, m=m: m(v).replace(f + " ", _alias.get(f, f) + " ", 1)), per) for (f, a, m, per) in G["muldiv"]]

# add and subtract of an immediate: rn may be sp for add and sub (not for the flag-setting forms' rd)
G["alu_imm"] = []
for sz in (64, 32):
    for n, m in (("add_ri", "add"), ("sub_ri", "sub")):
        for imm in (lit(0), lit(1), lit(4095), plus(mod(31, 97), 7)):
            G["alu_imm"].append(op(n, [lit(sz), I_, Q1, imm], lambda v, m=m: f"{m} {X(v[0],v[1],False)}, {X(v[0],v[2],False)}, #{v[3]}"))
    for n, m in (("add_ri12", "add"), ("sub_ri12", "sub")):
        G["alu_imm"].append(op(n, [lit(sz), I_, Q1, plus(mod(31, 100), 1)], lambda v, m=m: f"{m} {X(v[0],v[1],False)}, {X(v[0],v[2],False)}, #{v[3]}, lsl #12"))
    for n, m in (("adds_ri", "adds"), ("subs_ri", "subs")):
        for imm in (lit(0), lit(4095), plus(mod(31, 97), 7)):
            G["alu_imm"].append(op(n, [lit(sz), I_, Q1, imm], lambda v, m=m: f"{m} {X(v[0],v[1])}, {X(v[0],v[2],False)}, #{v[3]}"))
    G["alu_imm"].append(op("cmp_ri", [lit(sz), I_, plus(mod(31, 97), 3)], lambda v: f"cmp {X(v[0],v[1],False)}, #{v[2]}"))
G["alu_imm"] += [
    op("add_ri", [lit(64), lit(31), lit(31), lit(16)], lambda v: "add sp, sp, #16", per="once"),
    op("sub_ri", [lit(64), lit(31), lit(31), lit(32)], lambda v: "sub sp, sp, #32", per="once"),
    op("add_ri", [lit(64), lit(5), lit(31), lit(8)], lambda v: "add x5, sp, #8", per="once"),
    op("mov_sp", [lit(64), lit(31), lit(5)], lambda v: "mov sp, x5", per="once"),
    op("mov_sp", [lit(64), lit(5), lit(31)], lambda v: "mov x5, sp", per="once"),
    op("mov_sp", [lit(64), I_, Q1], lambda v: f"add {X(64,v[1],False)}, {X(64,v[2],False)}, #0"),
]

# shifts by immediate and extensions
G["shift_ext"] = []
for sz in (64, 32):
    for sh in ((1, 5, sz // 2, sz - 1)):
        G["shift_ext"].append(op("lsl_ri", [lit(sz), I_, Q1, lit(sh)], lambda v: f"lsl {X(v[0],v[1])}, {X(v[0],v[2])}, #{v[3]}"))
    for sh in ((0, 1, 5, sz // 2, sz - 1)):
        G["shift_ext"].append(op("lsr_ri", [lit(sz), I_, Q1, lit(sh)], lambda v: f"lsr {X(v[0],v[1])}, {X(v[0],v[2])}, #{v[3]}"))
        G["shift_ext"].append(op("asr_ri", [lit(sz), Q1, I_, lit(sh)], lambda v: f"asr {X(v[0],v[1])}, {X(v[0],v[2])}, #{v[3]}"))
    G["shift_ext"] += [
        op("sxtb", [lit(sz), I_, Q1], lambda v: f"sxtb {X(v[0],v[1])}, {X(32,v[2])}"),
        op("sxth", [lit(sz), Q1, I_], lambda v: f"sxth {X(v[0],v[1])}, {X(32,v[2])}"),
    ]
G["shift_ext"] += [
    op("sxtw", [I_, Q1], lambda v: f"sxtw {X(64,v[0])}, {X(32,v[1])}"),
    op("uxtb", [Q1, I_], lambda v: f"uxtb {X(32,v[0])}, {X(32,v[1])}"),
    op("uxth", [I_, Q1], lambda v: f"uxth {X(32,v[0])}, {X(32,v[1])}"),
]

# conditional select and compare
G["conditional"] = []
for sz in (64, 32):
    for n in ("csel", "csinc", "csinv", "csneg"):
        G["conditional"].append(op(n, [lit(sz), I_, Q1, Q2, nm(0)], lambda v, n=n: f"{n} {X(v[0],v[1])}, {X(v[0],v[2])}, {X(v[0],v[3])}, {CC[v[4]]}"))
    G["conditional"] += [
        op("cset", [lit(sz), I_, nm(3)], lambda v: f"cset {X(v[0],v[1])}, {CC[v[2]]}"),
        op("ccmp_rr", [lit(sz), I_, Q1, mod(16, 1), nm(5)], lambda v: f"ccmp {X(v[0],v[1])}, {X(v[0],v[2])}, #{v[3]}, {CC[v[4]]}"),
        op("ccmp_ri", [lit(sz), Q1, I_, mod(16, 1), nm(9)], lambda v: f"ccmp {X(v[0],v[1])}, #{v[2]}, #{v[3]}, {CC[v[4]]}"),
    ]

# memory
def memop(fn, regsz, asm, off, kind="imm"):
    return op(fn, [I_, B_, off], lambda v: asm(v))
G["memory"] = []
def mem(name, rt_sz, mn, offs, step, sz_arg=None):
    for off in offs:
        a = [I_, B_, off]
        if sz_arg is not None:
            a = [lit(sz_arg)] + a
            G["memory"].append(op(name, a, lambda v, mn=mn, rt=rt_sz: f"{mn} {X(v[0],v[1])}, [{X(64,v[2],False)}, #{v[3]}]"))
        else:
            G["memory"].append(op(name, a, lambda v, mn=mn, rt=rt_sz: f"{mn} {X(rt,v[0])}, [{X(64,v[1],False)}, #{v[2]}]"))
mem("str_x", 64, "str", (lit(0), lit(8), lit(32760), mod(16, 8)), 8)
mem("ldr_x", 64, "ldr", (lit(0), lit(8), lit(32760), mod(16, 8)), 8)
mem("str_w", 32, "str", (lit(0), lit(4), lit(16380), mod(16, 4)), 4)
mem("ldr_w", 32, "ldr", (lit(0), lit(4), lit(16380), mod(16, 4)), 4)
mem("strh", 32, "strh", (lit(0), lit(2), lit(8190), mod(16, 2)), 2)
mem("ldrh", 32, "ldrh", (lit(0), lit(2), lit(8190), mod(16, 2)), 2)
mem("strb", 32, "strb", (lit(0), lit(1), lit(4095), mod(16, 1)), 1)
mem("ldrb", 32, "ldrb", (lit(0), lit(1), lit(4095), mod(16, 1)), 1)
mem("ldrsw", 64, "ldrsw", (lit(0), lit(4), lit(16380), mod(16, 4)), 4)
for sz in (64, 32):
    mem("ldrsb", sz, "ldrsb", (lit(0), lit(1), lit(4095), mod(16, 1)), 1, sz)
    mem("ldrsh", sz, "ldrsh", (lit(0), lit(2), lit(8190), mod(16, 2)), 2, sz)
for name, mn, rtsz in (("stur_x", "stur", 64), ("ldur_x", "ldur", 64), ("stur_w", "stur", 32), ("ldur_w", "ldur", 32)):
    for off in (lit(-256), lit(-1), lit(255), plus(I_, -15)):
        G["memory"].append(op(name, [I_, B_, off], lambda v, mn=mn, rt=rtsz: f"{mn} {X(rt,v[0])}, [{X(64,v[1],False)}, #{v[2]}]"))
# register offsets, the 32-bit address zero-extended (uxtw) and a 64-bit register
def regform(name, rtsz, mn, ext, sz_arg=None):
    a = [I_, B_, Q2]
    if sz_arg is not None:
        a = [lit(sz_arg)] + a
        G["memory"].append(op(name, a, lambda v, mn=mn: f"{mn} {X(v[0],v[1])}, [{X(64,v[2],False)}, {X(32 if ext else 64, v[3])}{', uxtw' if ext else ''}]"))
    else:
        G["memory"].append(op(name, a, lambda v, mn=mn, rt=rtsz: f"{mn} {X(rt,v[0])}, [{X(64,v[1],False)}, {X(32 if ext else 64, v[2])}{', uxtw' if ext else ''}]"))
regform("ldr_x_uxtw", 64, "ldr", True); regform("str_x_uxtw", 64, "str", True)
regform("ldr_w_uxtw", 32, "ldr", True); regform("str_w_uxtw", 32, "str", True)
regform("ldrh_uxtw", 32, "ldrh", True); regform("strh_uxtw", 32, "strh", True)
regform("ldrb_uxtw", 32, "ldrb", True); regform("strb_uxtw", 32, "strb", True)
regform("ldrsw_uxtw", 64, "ldrsw", True)
for sz in (64, 32):
    regform("ldrsb_uxtw", sz, "ldrsb", True, sz); regform("ldrsh_uxtw", sz, "ldrsh", True, sz)
regform("ldr_x_reg", 64, "ldr", False); regform("str_x_reg", 64, "str", False)
# pairs
for off in (lit(-512), lit(0), lit(8), lit(504), mod(8, 8)):
    for name, mn in (("stp", "stp"), ("ldp", "ldp")):
        G["memory"].append(op(name, [I_, N1, B_, off], lambda v, mn=mn: f"{mn} {X(64,v[0])}, {X(64,v[1])}, [{X(64,v[2],False)}, #{v[3]}]"))
for off in (lit(-16), lit(16), lit(-512), lit(504)):
    for name, mn in (("stp_pre", "stp"), ("ldp_pre", "ldp")):
        G["memory"].append(op(name, [I_, N1, B_, off], lambda v, mn=mn: f"{mn} {X(64,v[0])}, {X(64,v[1])}, [{X(64,v[2],False)}, #{v[3]}]!"))
    for name, mn in (("stp_post", "stp"), ("ldp_post", "ldp")):
        G["memory"].append(op(name, [I_, N1, B_, off], lambda v, mn=mn: f"{mn} {X(64,v[0])}, {X(64,v[1])}, [{X(64,v[2],False)}], #{v[3]}"))
# single registers with writeback: a stack's push and pop
for off in (lit(-16), lit(16), lit(-256), lit(255), plus(mod(31, 8), -9)):
    for name, mn in (("str_x_pre", "str"), ("ldr_x_pre", "ldr")):
        G["memory"].append(op(name, [I_, B_, off], lambda v, mn=mn: f"{mn} {X(64,v[0])}, [{X(64,v[1],False)}, #{v[2]}]!"))
    for name, mn in (("str_x_post", "str"), ("ldr_x_post", "ldr")):
        G["memory"].append(op(name, [I_, B_, off], lambda v, mn=mn: f"{mn} {X(64,v[0])}, [{X(64,v[1],False)}], #{v[2]}"))
# the stack pointer as a base, once
# the float registers (s and d): scaled, unscaled, and register offsets
for ft, offs in ((1, (lit(0), lit(8), lit(32760), mod(16, 8))), (0, (lit(0), lit(4), lit(16380), mod(16, 4)))):
    for name, mn in (("ldr_fp", "ldr"), ("str_fp", "str")):
        for off in offs:
            G["memory"].append(op(name, [lit(ft), I_, B_, off], lambda v, mn=mn: f"{mn} {V(v[0],v[1])}, [{X(64,v[2],False)}, #{v[3]}]"))
    for name, mn in (("ldur_fp", "ldur"), ("stur_fp", "stur")):
        for off in (lit(-256), lit(-1), lit(255), plus(I_, -15)):
            G["memory"].append(op(name, [lit(ft), I_, B_, off], lambda v, mn=mn: f"{mn} {V(v[0],v[1])}, [{X(64,v[2],False)}, #{v[3]}]"))
    for name, mn in (("ldr_fp_uxtw", "ldr"), ("str_fp_uxtw", "str")):
        G["memory"].append(op(name, [lit(ft), I_, B_, Q2], lambda v, mn=mn: f"{mn} {V(v[0],v[1])}, [{X(64,v[2],False)}, {X(32,v[3])}, uxtw]"))
G["memory"] += [
    op("str_x", [lit(5), lit(31), lit(16)], lambda v: "str x5, [sp, #16]", per="once"),
    op("ldr_x", [lit(5), lit(31), lit(8)], lambda v: "ldr x5, [sp, #8]", per="once"),
    op("stp_pre", [lit(29), lit(30), lit(31), lit(-16)], lambda v: "stp x29, x30, [sp, #-16]!", per="once"),
    op("ldp_post", [lit(29), lit(30), lit(31), lit(16)], lambda v: "ldp x29, x30, [sp], #16", per="once"),
    op("str_x_pre", [lit(0), lit(31), lit(-16)], lambda v: "str x0, [sp, #-16]!", per="once"),
    op("ldr_x_post", [lit(0), lit(31), lit(16)], lambda v: "ldr x0, [sp], #16", per="once"),
    op("ldr_x_uxtw", [lit(5), lit(28), lit(6)], lambda v: "ldr x5, [x28, w6, uxtw]", per="once"),
    op("str_w_uxtw", [lit(5), lit(28), lit(31)], lambda v: "str w5, [x28, wzr, uxtw]", per="once"),
]

# floating point: the registers v0-v30 as s (ft 0) and d (ft 1)
def V(ft, n): return f"{'d' if ft else 's'}{n}"
G["float"] = []
for ft in (0, 1):
    G["float"] += [
        op("fmov_to_fp", [lit(ft), I_, Q1], lambda v: f"fmov {V(v[0],v[1])}, {X(64 if v[0] else 32, v[2])}"),
        op("fmov_from_fp", [lit(ft), Q1, I_], lambda v: f"fmov {X(64 if v[0] else 32, v[1])}, {V(v[0],v[2])}"),
        op("fcmp", [lit(ft), I_, Q1], lambda v: f"fcmp {V(v[0],v[1])}, {V(v[0],v[2])}"),
    ]
    for n in ("fmul", "fdiv", "fadd", "fsub", "fmax", "fmin"):
        G["float"].append(op(n, [lit(ft), I_, Q1, Q2], lambda v, n=n: f"{n} {V(v[0],v[1])}, {V(v[0],v[2])}, {V(v[0],v[3])}"))
    for n, m in (("fmov_rr", "fmov"), ("fabs", "fabs"), ("fneg", "fneg"), ("fsqrt", "fsqrt"), ("frintn", "frintn"), ("frintp", "frintp"), ("frintm", "frintm"), ("frintz", "frintz")):
        G["float"].append(op(n, [lit(ft), Q1, I_], lambda v, m=m: f"{m} {V(v[0],v[1])}, {V(v[0],v[2])}"))
    for sz in (32, 64):
        for n in ("fcvtzs", "fcvtzu"):
            G["float"].append(op(n, [lit(sz), lit(ft), I_, Q1], lambda v, n=n: f"{n} {X(v[0],v[2])}, {V(v[1],v[3])}"))
        for n in ("scvtf", "ucvtf"):
            G["float"].append(op(n, [lit(sz), lit(ft), I_, Q1], lambda v, n=n: f"{n} {V(v[1],v[2])}, {X(v[0],v[3])}"))
G["float"] += [
    op("fcvt_to_double", [I_, Q1], lambda v: f"fcvt {V(1,v[0])}, {V(0,v[1])}"),
    op("fcvt_to_single", [Q1, I_], lambda v: f"fcvt {V(0,v[0])}, {V(1,v[1])}"),
]

# acquire/release and exclusive access, the base register never the register moved
G["atomic"] = []
for sz in (64, 32):
    for n in ("ldar", "stlr", "ldaxr"):
        G["atomic"].append(op(n, [lit(sz), I_, B_], lambda v, n=n: f"{n} {X(v[0],v[1])}, [{X(64,v[2],False)}]"))
    G["atomic"].append(op("stlxr", [lit(sz), N1, I_, B_], lambda v: f"stlxr {X(32,v[1])}, {X(v[0],v[2])}, [{X(64,v[3],False)}]"))
G["atomic"].append(op("clrex", [], lambda v: "clrex", per="once"))

# mov_imm: the movz/movn/movk sequence it writes, for these values
MOV_VALUES = [0, 1, 65535, 65536, 4294967295, -1, -2, -65536, -65537, 305419896, 81985529216486895,
              -81985529216486896, 4886718345, 0x0000ffff0000ffff, 0xffff0000ffff0000 - (1 << 64), 0x00010000_00010000, 281474976710656]
def mov_imm_seq(sz, rd, v):
    n = 4 if sz == 64 else 2
    mask = (1 << sz) - 1
    u = v & mask
    half = lambda k: (u >> (16 * k)) & 0xffff
    zeros = sum(1 for k in range(n) if half(k) == 0)
    ones = sum(1 for k in range(n) if half(k) == 0xffff)
    fill = 0xffff if ones > zeros else 0
    first = next((k for k in range(n) if half(k) != fill), -1)
    r = X(sz, rd)
    if first < 0:
        return [f"movz {r}, #0" if fill == 0 else f"movn {r}, #0"], u
    lines = []
    if fill == 0:
        lines.append(f"movz {r}, #{half(first)}" + (f", lsl #{16*first}" if first else ""))
    else:
        lines.append(f"movn {r}, #{half(first) ^ 0xffff}" + (f", lsl #{16*first}" if first else ""))
    for k in range(first + 1, n):
        if half(k) != fill:
            lines.append(f"movk {r}, #{half(k)}" + (f", lsl #{16*k}" if k else ""))
    # what the sequence builds
    val = 0xffffffffffffffff if fill else 0
    val &= mask
    cur = val
    for ln in lines:
        m = re.match(r"(mov[znk]) \w+, #(\d+)(?:, lsl #(\d+))?", ln)
        k, imm, sh = m.group(1), int(m.group(2)), int(m.group(3) or 0)
        if k == "movz": cur = imm << sh
        elif k == "movn": cur = (~(imm << sh)) & mask
        else: cur = (cur & ~(0xffff << sh)) | (imm << sh)
    assert cur == u, (sz, v, lines, hex(cur), hex(u))
    return lines, u
G["moves"] = []
for sz in (64, 32):
    G["moves"] += [
        op("movz", [lit(sz), I_, mod(31, 1000), mod(sz // 16, 16)], lambda v: f"movz {X(v[0],v[1])}, #{v[2]}" + (f", lsl #{v[3]}" if v[3] else "")),
        op("movk", [lit(sz), Q1, plus(mod(31, 1000), 7), mod(sz // 16, 16)], lambda v: f"movk {X(v[0],v[1])}, #{v[2]}" + (f", lsl #{v[3]}" if v[3] else "")),
        op("movn", [lit(sz), Q2, plus(mod(31, 1000), 3), mod(sz // 16, 16)], lambda v: f"movn {X(v[0],v[1])}, #{v[2]}" + (f", lsl #{v[3]}" if v[3] else "")),
        op("movz", [lit(sz), I_, lit(65535), lit(0)], lambda v: f"movz {X(v[0],v[1])}, #65535"),
    ]
MOV_IMM = [(sz, v) for sz in (64, 32) for v in MOV_VALUES if sz == 64 or -(1 << 31) <= v < (1 << 32)]

def aipl_i64(v):
    return f"{v}i64"

# ---- branches, system: fixed code, with labels ----
BRANCH_AIPL = '''  (fn test_branches [] -> i32
    (let c:(ptr buf.Buf) (call buf.make (call alloc.default) 2048))
    (let start:i32 (call buf.len c))
    (let fwd:(ptr (vec.Vec i32)) (call (vec.make i32) (call alloc.default) 32))
    (call (vec.push i32) fwd (call b c))
    (call (vec.push i32) fwd (call bl c))
    (for cc 0 14 (call (vec.push i32) fwd (call b_cond c cc)))
    (call (vec.push i32) fwd (call cbz c 64 3))
    (call (vec.push i32) fwd (call cbnz c 64 4))
    (call (vec.push i32) fwd (call cbz c 32 5))
    (call (vec.push i32) fwd (call cbnz c 32 30))
    (call b_to c start)
    (let adr_at:i32 (call adr c 7))
    (call br c 9)
    (call blr c 10)
    (call ret_r c 11)
    (call ret c)
    (let end:i32 (call buf.len c))
    (call ret c)
    (for k 0 (call (vec.len i32) fwd)
      (let at:i32 (call (vec.at i32) fwd k))
      (if (lt k 2) (call patch_b c at end) (call patch_cond c at end)))
    (call patch_adr c adr_at end)
    (call check "branches" c "BRANCHES"))

  (fn test_system [] -> i32
    (let c:(ptr buf.Buf) (call buf.make (call alloc.default) 128))
    (call svc c 0)
    (call svc c 65535)
    (call brk c 0)
    (call brk c 1000)
    (call udf c 0)
    (call udf c 513)
    (call nop c)
    (call dmb_ish c)
    (call isb c)
    (call mrs_tpidr c 0)
    (call mrs_tpidr c 17)
    (call msr_tpidr c 1)
    (call msr_tpidr c 30)
    (call check "system" c "SYSTEM"))
'''
BRANCH_ASM = ["start:", "b end", "bl end"] + [f"b.{c} end" for c in "eq ne hs lo mi pl vs vc hi ls ge lt gt le".split()] + \
    ["cbz x3, end", "cbnz x4, end", "cbz w5, end", "cbnz w30, end", "b start", "adr x7, end", "br x9", "blr x10", "ret x11", "ret", "end:", "ret"]
SYSTEM_ASM = ["svc #0", "svc #65535", "brk #0", "brk #1000", "udf #0", "udf #513", "nop", "dmb ish", "isb",
              "mrs x0, tpidr_el0", "mrs x17, tpidr_el0", "msr tpidr_el0, x1", "msr tpidr_el0, x30"]

def aipl_group(name, extra=()):
    """The AIPL test function for a group, with its expected hex as HEX_<name>."""
    ops = G[name]
    loops = [o for o in ops if o[3] == "i"]
    once = [o for o in ops if o[3] == "once"]
    def call(o):
        fn, args, asm, per = o
        return f"(call {fn} c " + " ".join(a[0] for a in args) + ")"
    lines = [f"      {call(o)}" for o in loops] + list(extra)
    lines[-1] += ")"
    body = [f"  (fn test_{name} [] -> i32", "    (let c:(ptr buf.Buf) (call buf.make (call alloc.default) 65536))", "    (for i 0 31"]
    body += lines
    body += [f"    {call(o)}" for o in once]
    body.append(f'    (call check "{name}" c "HEX_{name}"))')
    return "\n".join(body) + "\n"

def asm_group(name):
    lines = []
    for i in range(31):
        for fn, args, asm, per in G[name]:
            if per == "i":
                lines.append(asm([a[1](i) for a in args]))
    for fn, args, asm, per in G[name]:
        if per == "once":
            lines.append(asm([a[1](0) for a in args]))
    return lines

def moves_asm():
    lines = []
    for i in range(31):
        for fn, args, asm, per in G["moves"]:
            lines.append(asm([a[1](i) for a in args]))
        for sz, v in MOV_IMM:
            lines += mov_imm_seq(sz, i, v)[0]
    return lines

def build():
    groups = ["moves", "alu_reg", "muldiv", "alu_imm", "shift_ext", "conditional", "memory", "float", "atomic"]
    asm = {n: (moves_asm() if n == "moves" else asm_group(n)) for n in groups}
    asm["branches"] = BRANCH_ASM
    asm["system"] = SYSTEM_ASM
    return groups, asm

def assemble(lines_by_group, host, local):
    """The hex of each group, assembled together: one object file per group"""
    hexes = {}
    for name, lines in lines_by_group.items():
        text = "\n".join(lines) + "\n"
        if local:
            d = tempfile.mkdtemp()
            open(f"{d}/t.s", "w").write(text)
            subprocess.run(["clang", "-c", "-arch", "arm64", f"{d}/t.s", "-o", f"{d}/t.o"], check=True)
            out = subprocess.run(["otool", "-tX", f"{d}/t.o"], capture_output=True, text=True, check=True).stdout
        else:
            script = "set -e; d=$(mktemp -d); cat > $d/t.s; clang -c -arch arm64 $d/t.s -o $d/t.o; otool -tX $d/t.o; rm -rf $d"
            r = subprocess.run(["ssh", "-o", "BatchMode=yes", host, script], input=text, capture_output=True, text=True)
            if r.returncode != 0:
                sys.exit(f"assembling {name} failed:\n{r.stderr}")
            out = r.stdout
        words = []
        for ln in out.splitlines():
            parts = ln.split()
            words += parts[1:]
        hexes[name] = "".join(words)
    return hexes

def region(hexes, groups):
    parts = ["""  ;; ARM64_TESTS_BEGIN
  ;; Test vectors are the output of Apple's assembler (clang -arch arm64, on a
  ;; Mac, read back with otool) for the same instruction sequences, pasted in
  ;; as hex; they are never produced by this encoder (python3
  ;; tools/arm64_vectors.py regenerates this whole region). Each loop covers every
  ;; register x0-x30. q(r) = (7r + 3) mod 31 pairs each register with a
  ;; different one, so every register also appears in the other operands.
  (fn q [r:i32] -> i32 (% (+ (* r 7) 3) 31))
"""]
    for n in groups:
        extra = [f"      (call mov_imm c {sz} i {aipl_i64(v)})" for sz, v in MOV_IMM] if n == "moves" else ()
        parts.append(aipl_group(n, extra).replace(f"HEX_{n}", hexes[n]))
    parts.append(BRANCH_AIPL.replace("BRANCHES", hexes["branches"]).replace("SYSTEM", hexes["system"]))
    names = groups + ["branches", "system"]
    expr = f"(call test_{names[-1]})"
    for n in reversed(names[:-1]):
        expr = f"(+ (call test_{n}) {expr})"
    parts.append(f"  (fn run_arm64_tests [] -> i32 (pub)\n    {expr})\n  ;; ARM64_TESTS_END\n")
    return "\n".join(parts), len(names)

def main():
    host, local, check = "pennylane", False, False
    a = sys.argv[1:]
    while a:
        x = a.pop(0)
        if x == "--host": host = a.pop(0)
        elif x == "--local": local = True
        elif x == "--check": check = True
    groups, asm = build()
    hexes = assemble(asm, host, local)
    text, count = region(hexes, groups)
    src = open(TARGET).read()
    b, e = src.index("  ;; ARM64_TESTS_BEGIN"), src.index("  ;; ARM64_TESTS_END\n") + len("  ;; ARM64_TESTS_END\n")
    new = src[:b] + text + src[e:]
    if check:
        sys.exit(0 if new == src else "arm64.aipl's test region is not up to date: run tools/arm64_vectors.py")
    open(TARGET, "w").write(new)
    print(f"{count} groups, {sum(len(h) for h in hexes.values()) // 8} words")

if __name__ == "__main__":
    main()
