#!/usr/bin/env python3
"""Runs a command on a system without openat2, as Linux before 5.6, Rosetta 2, and
container profiles that refuse it are (docs/HANDOFF.md, the openat2 fallback).

    without_openat2.py [--eperm] COMMAND [ARG...]

Installs a seccomp filter that makes the openat2 system call fail with ENOSYS
(with EPERM, as an old Docker profile does, under --eperm), then runs the
command, whose children keep the filter, and sets WITHOUT_OPENAT2=1 for it. Linux
x86-64 only; no privileges needed.
"""
import ctypes
import os
import struct
import sys

PR_SET_NO_NEW_PRIVS = 38
PR_SET_SECCOMP = 22
SECCOMP_MODE_FILTER = 2
AUDIT_ARCH_X86_64 = 0xC000003E
SYS_OPENAT2 = 437
SECCOMP_RET_ALLOW = 0x7FFF0000
SECCOMP_RET_ERRNO = 0x00050000
BPF_LD_W_ABS, BPF_JEQ_K, BPF_RET_K = 0x20, 0x15, 0x06


def insn(code, jt, jf, k):
    return struct.pack("HBBI", code, jt, jf, k)


def install(errno):
    program = b"".join([
        insn(BPF_LD_W_ABS, 0, 0, 4),                      # the architecture
        insn(BPF_JEQ_K, 1, 0, AUDIT_ARCH_X86_64),         # x86-64: go on; else allow
        insn(BPF_RET_K, 0, 0, SECCOMP_RET_ALLOW),
        insn(BPF_LD_W_ABS, 0, 0, 0),                      # the system call number
        insn(BPF_JEQ_K, 0, 1, SYS_OPENAT2),               # openat2: fail; else allow
        insn(BPF_RET_K, 0, 0, SECCOMP_RET_ERRNO | errno),
        insn(BPF_RET_K, 0, 0, SECCOMP_RET_ALLOW),
    ])
    libc = ctypes.CDLL(None, use_errno=True)
    buf = ctypes.create_string_buffer(program, len(program))

    class Fprog(ctypes.Structure):
        _fields_ = [("len", ctypes.c_ushort), ("filter", ctypes.c_void_p)]

    prog = Fprog(len(program) // 8, ctypes.cast(buf, ctypes.c_void_p))
    if libc.prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 or libc.prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, ctypes.byref(prog), 0, 0) != 0:
        sys.exit(f"without_openat2: cannot install the filter: {os.strerror(ctypes.get_errno())}")


def main():
    args = sys.argv[1:]
    errno = 38
    if args[:1] == ["--eperm"]:
        errno, args = 1, args[1:]
    if not args:
        sys.exit(__doc__)
    if sys.platform != "linux" or os.uname().machine != "x86_64":
        sys.exit("without_openat2: Linux x86-64 only")
    install(errno)
    os.environ["WITHOUT_OPENAT2"] = "1"  # for tests that expect the difference (tests/test_native.rs)
    os.execvp(args[0], args)


if __name__ == "__main__":
    main()
