#!/usr/bin/env python3
"""Test vectors for aipl_src/native/x64.aipl (native backend task NE3).

Each group below mirrors the loops of the matching `test_<group>` function
in x64.aipl, written as Intel-syntax assembly and assembled with GNU as
(binutils). The resulting bytes are printed as hex, to paste into
`(call check "<group>" c "<hex>")`. The tests do not run this script; it is
here so the vectors can be regenerated or extended (SSE2 forms in NE13).

    python3 tools/x64_vectors.py            # every group
    python3 tools/x64_vectors.py memory     # one group
"""
import subprocess, sys, tempfile
R64="rax rcx rdx rbx rsp rbp rsi rdi r8 r9 r10 r11 r12 r13 r14 r15".split()
R32="eax ecx edx ebx esp ebp esi edi r8d r9d r10d r11d r12d r13d r14d r15d".split()
R8="al cl dl bl spl bpl sil dil r8b r9b r10b r11b r12b r13b r14b r15b".split()
CC="o no b ae e ne be a s ns p np l ge le g".split()
def R(sz,r): return (R64 if sz==64 else R32)[r]
def P(r): return (r*7+3)&15
def I(b):
    i=(b*3+1)&15
    return 9 if i==4 else i
def S(b): return [1,2,4,8][b&3]
def mem(sz,b,d,idx=None,s=1):
    w={8:"byte",32:"dword",64:"qword",0:""}[sz]
    a=R64[b]+(f"+{R64[idx]}*{s}" if idx is not None else "")+(f"{d:+d}" if d else "")
    return (f"{w} ptr " if w else "")+f"[{a}]"
ALU=[(0,"add"),(1,"or"),(4,"and"),(5,"sub"),(6,"xor"),(7,"cmp")]
SH=[(4,"shl"),(5,"shr"),(7,"sar")]
def moves():
    o=[]
    for r in range(16):
        o+= [f"mov {R(64,r)}, {R(64,P(r))}", f"mov {R(32,r)}, {R(32,P(r))}",
             f"mov {R(32,r)}, -5", f"mov {R(64,r)}, -5", f"mov {R(64,r)}, 0x123456789",
             f"movsxd {R(64,r)}, {R(32,P(r))}", f"movzx {R(32,r)}, {R8[P(r)]}",
             f"push {R(64,r)}", f"pop {R(64,r)}", f"call {R(64,r)}",
             f"mov {R(64,r)}, 2147483647", f"mov {R(64,r)}, -2147483648", f"movabs {R(64,r)}, 2147483648", f"movabs {R(64,r)}, -2147483649"]
    return o
def alu():
    o=[]
    for op,n in ALU:
        for r in range(16):
            o+=[f"{n} {R(32,r)}, {R(32,P(r))}", f"{n} {R(64,P(r))}, {R(64,r)}",
                f"{n} {R(32,r)}, 5", f"{n} {R(64,r)}, -100", f"{n} {R(32,r)}, 1000", f"{n} {R(64,r)}, -70000",
                f"{n} {R(32,r)}, 127", f"{n} {R(64,r)}, -128", f"{n} {R(32,r)}, 128", f"{n} {R(64,r)}, -129"]
    for r in range(16):
        o+=[f"test {R(32,r)}, {R(32,P(r))}", f"test {R(64,r)}, {R(64,P(r))}", f"test {R(32,r)}, 255", f"test {R(64,r)}, 65536"]
    return o
def muldiv():
    o=[]
    for r in range(16):
        for k,n in SH:
            o+=[f"{n} {R(32,r)}, 1", f"{n} {R(64,r)}, 5", f"{n} {R(32,r)}, 31", f"{n} {R(64,r)}, 63", f"{n} {R(64,r)}, cl", f"{n} {R(32,r)}, cl"]
        o+=[f"imul {R(32,r)}, {R(32,P(r))}", f"imul {R(64,r)}, {R(64,P(r))}",
            f"div {R(32,r)}", f"div {R(64,r)}", f"idiv {R(32,r)}", f"idiv {R(64,r)}", "cdq", "cqo",
            f"set{CC[r]} {R8[r]}", f"set{CC[(r+5)&15]} {R8[P(r)]}"]
    return o
def memory():
    o=[]
    for b in range(16):
        p=P(b); i=I(b); s=S(b)
        o+=[f"mov {R(32,p)}, {mem(32,b,0)}", f"mov {R(64,p)}, {mem(64,b,8)}",
            f"mov {mem(32,b,-200)}, {R(32,p)}", f"mov {mem(64,b,100000)}, {R(64,p)}",
            f"mov {mem(8,b,1)}, {R8[p]}", f"movzx {R(32,p)}, {mem(8,b,-1)}", f"lea {R(64,p)}, {mem(0,b,16)}",
            f"mov {R(64,p)}, {mem(64,b,0,i,s)}", f"mov {mem(32,b,-8,i,s)}, {R(32,p)}",
            f"lea {R(32,p)}, {mem(0,b,1024,i,s)}", f"mov {mem(8,b,127,i,s)}, {R8[p]}",
            f"movzx {R(32,p)}, {mem(8,b,128,i,s)}"]
    return o
def atomics():
    o=[]
    for b in range(16):
        p=P(b); i=I(b); s=S(b)
        o+=[f"lock xadd {mem(32,b,0)}, {R(32,p)}", f"lock xadd {mem(64,b,8,i,s)}, {R(64,p)}",
            f"xchg {mem(32,b,4,i,s)}, {R(32,p)}", f"xchg {mem(64,b,0)}, {R(64,p)}",
            f"lock cmpxchg {mem(32,b,-4)}, {R(32,p)}", f"lock cmpxchg {mem(64,b,0,i,s)}, {R(64,p)}"]
    return o
def control():
    o=["start:","{disp32} jmp L1","{disp32} je L1","{disp32} call start","ud2","L1: ret","syscall"]
    o+=[f"{{disp32}} j{CC[k]} start" for k in range(16)]
    o+=["{disp32} jmp start"]
    return o
def strings():
    return ["rep movsb"]
def assemble(lines):
    d=tempfile.mkdtemp()
    open(f"{d}/a.s","w").write(".intel_syntax noprefix\n"+"\n".join(lines)+"\n")
    subprocess.run(["as",f"{d}/a.s","-o",f"{d}/a.o"],check=True)
    subprocess.run(["objcopy","-O","binary","-j",".text",f"{d}/a.o",f"{d}/a.bin"],check=True)
    return open(f"{d}/a.bin","rb").read().hex()
GROUPS = [("moves", moves), ("alu", alu), ("muldiv", muldiv), ("memory", memory), ("atomics", atomics), ("control", control), ("strings", strings)]

if __name__ == "__main__":
    wanted = sys.argv[1:]
    for name, f in GROUPS:
        if not wanted or name in wanted:
            print(f"{name}: {assemble(f())}")
