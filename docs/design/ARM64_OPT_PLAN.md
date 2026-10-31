# The AArch64 backend's optimizer

Status: **phase 1 done** (2026-10-30); phases 2 and 3 to come. The baseline AArch64 backend (docs/design/ARM64_PLAN.md) does everything
the x86-64 one does and is slow: every wasm value is a 16-byte slot on the machine stack, every local is a
frame slot, and a float operation moves its operands through integer registers. This plan makes it fast.
The goal is the x86-64 backend's distance from C, not C's speed.

## Where it stands

The eight benchmarks in `benchmarks/` (the C versions at `-O2`, natively on the same Apple M4; the AIPL
programs built by the backend named and run in a `linux/arm64` container on it), best of three, as of
2026-10-30:

| benchmark | C -O2 | AIPL AArch64 now | ratio | AIPL x86-64 (its machine) | ratio |
|---|---|---|---|---|---|
| binarytrees 16 | 0.14 s | 2.46 s | 17.6x | 0.25 s | 2.0x |
| fannkuch 10 | 0.12 s | 4.46 s | 36.6x | 0.29 s | 2.0x |
| knucleotide | 0.01 s | 0.12 s | 16.0x | 0.01 s | 2.8x |
| mandelbrot 1000 | 0.03 s | 0.50 s | 16.4x | 0.06 s | 1.3x |
| nbody 1000000 | 0.03 s | 1.62 s | 50.7x | 0.12 s | 3.4x |
| spectralnorm 1000 | 0.03 s | 0.79 s | 28.8x | 0.06 s | 1.7x |
| spigot 10000 | 0.93 s | 10.64 s | 11.4x | 2.02 s | 1.4x |

So nearly all of the AArch64 gap is the backend, not the language: x86-64 pays 1.3 to 3.4 times C for the
same wasm. The float-heavy programs are worst (nbody), which fits the cause: a float operation in the
baseline is a load into an integer register, `fmov` to a float register, the operation, `fmov` back, and a
store, and each store-to-load round trip costs several cycles on the critical path.

## The design

The same as the x86-64 backend's (docs/design/NATIVE_SPEED_PLAN.md), because it is what works:

1. **A virtual operand stack.** The wasm stack's top lives in registers, constants, or locals' homes, not in
   memory: an entry says where its value is (an integer register, a float register, an i32 or i64 constant, a
   local, or a comparison's result in the flags). An instruction takes its operands where they are, so a
   constant is an immediate and a local is read in place. A register is taken from a pool when a value needs
   one; when none is free the deepest register value is spilled to the stack. At every label, branch, call,
   and return the stack is flushed: its values go to their slots in order, which is exactly what control
   flow expects. Structured wasm has an empty operand stack at most branch targets, so the flush costs
   little.
2. **Typed registers.** Floats stay in `d`/`s` registers from load to store; no instruction moves a float
   through an integer register but `fmov` at a conversion or a reinterpret.
3. **Locals in registers.** The locals a function uses most (counted, loops weighted) live in callee-saved
   registers (`x19`-`x25`, and `d8`-`d15` for floats), the rest in frame slots. A function saves the
   callee-saved registers it uses. The routines for imports (`wasi_arm64`, `procs_arm64`, `sockets_arm64`,
   the trap routines) use `x19`-`x27` freely today, so they save and restore `x19`-`x25` (and use no `d8`-`d15`).
4. **Compare and branch fused.** A comparison whose result the next instruction branches on or selects
   with sets the flags and branches, with no `cset`.
5. **Addressing.** A load or store is `ldr [x28, wN, uxtw]` when its offset is 0, and `add x16, x28, wN,
   uxtw` then `ldr [x16, #off]` otherwise (the unsigned scaled immediate when the offset suits the width,
   `ldur` for a small unaligned one); an offset that cannot be folded costs one `mov`.
6. **Calling convention** (phase 3): wasm functions called from wasm take their first eight arguments in
   `x0`-`x7` / `d0`-`d7`, and return in `x0`/`d0`. The routines for imports keep the stack convention, which
   a call to an import (known at the call site by its index) still uses; so does `call_indirect` until the
   table holds only functions with the register convention.

What stays: the frame (`x29`/`x30`, so the stack walk of a trap works), the trap routines and their `bl` call
convention, the memory base in `x28`, the position and function tables (every function's range is still its
own, so the call chain is the same), and the unsupported-instruction message.

## Phase 1: done (2026-10-30)

The virtual operand stack is in `lower_arm64.aipl`: entries in integer pool registers `x9`-`x15`, float pool
registers `v16`-`v31`, constants, and the flags of a comparison; `pop` and `push` keep their meaning, so the
integer, atomic, and memory emitters did not change, and the float arithmetic, comparisons, loads, and stores,
the locals (typed: a float local is loaded into a float register), the constants, `select`, `drop`, `eqz`,
`if` and `br_if` were rewritten to use it. The stack is flushed to the machine stack at every block, loop,
else, end, branch, return, and call, and when the pool is full the deepest entry is spilled. The emitters that
used `x9`-`x15` (notify, the futex wait, `memory.init`, `memory.grow`) use other scratch registers now.
The encoder gained the float loads and stores (checked against Apple's assembler).

Measured the same way as the table above (best of three):

| benchmark | C -O2 | before | after | ratio before | ratio after |
|---|---|---|---|---|---|
| binarytrees 16 | 0.18 s | 2.46 s | 0.50 s | 17.6x | 2.9x |
| fannkuch 10 | 0.12 s | 4.46 s | 1.00 s | 36.6x | 8.2x |
| knucleotide | 0.01 s | 0.12 s | 0.05 s | 16.0x | 7.7x |
| mandelbrot 1000 | 0.03 s | 0.50 s | 0.22 s | 16.4x | 7.1x |
| nbody 1000000 | 0.03 s | 1.62 s | 0.44 s | 50.7x | 13.8x |
| spectralnorm 1000 | 0.03 s | 0.79 s | 0.19 s | 28.8x | 6.9x |
| spigot 10000 | 0.93 s | 10.64 s | 2.23 s | 11.4x | 2.4x |

All eight benchmarks print their `expected.txt`; the compiler built this way compiles itself to the same bytes
on AArch64 Linux and every repository program to the Rust command's bytes (`tools/arm64_bootstrap.sh`, which
now takes 1m43s instead of 3 minutes: the compiler itself runs faster there).

## Phases, each measured against the table above

1. **The virtual stack, constants, typed registers, fused compare and branch, addressing** (locals still in
   frame slots). Expected: 3 to 5 times faster.
2. **Locals in registers**, with the saves in the import routines. Expected: another 2 to 3 times.
3. **The register calling convention.** Expected: 1.2 to 1.5 times on call-heavy programs (binarytrees).

A phase is done when every benchmark's output is unchanged, `tests/test_native_arm64*.rs` and
`tools/arm64_bootstrap.sh` pass (the compiler built by the optimized backend compiles itself to the same
bytes and every repository program to the Rust command's bytes), the corpus of `tests/aipl` runs
byte-identical to the x86-64 build's, and the table is re-measured.

## Risks

- A wrong register allocation is a wrong program, not a slow one: the bootstrap's fixed point (the
  compiler compiling itself under the optimized code), the 107-program corpus, and
  `tools/arm64_fuzz.py` (the generated programs of `tools/run_fuzz.py` on AArch64 against x86-64: 3,000 cases
  at the end of phase 1, with trap messages compared, and a planted fault in float subtraction fails 41 of
  300) are the net. The one difference it accepts is the sign of a default NaN (a 0/0 is negative on x86-64
  and positive on AArch64; wasm leaves it open).
- The seed (`bootstrap/aipl-linux-aarch64`) contains the baseline backend. Its stage 2 is the optimized
  compiler, which differs from the seed until the seed is refreshed; that is the normal state, and is
  checked by `tools/arm64_bootstrap.sh` without `SEED_FROM_SOURCE`.
- Code size grows with the saves and the spills; the compiler is 7.9 MB now.
