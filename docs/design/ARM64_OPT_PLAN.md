# The AArch64 backend's optimizer

Status: **phases 1, 2, 3, and 4 done** (2026-10-30). The baseline AArch64 backend (docs/design/ARM64_PLAN.md) does everything
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

## Phase 2: done (2026-10-30)

The locals a function uses most live in registers: integers in `x19`-`x25`, floats in `d8`-`d15`, chosen by use
count with each enclosing loop weighting a use 8 times (to 4096). A function saves the ones it uses in a save
area just below its frame pointer and restores them at every return; its declared locals still have frame slots
(so `sp` arithmetic is unchanged), but only the ones in memory are zeroed there. A parameter that gets a register
is loaded in the prologue. A `local.get` of a register local is a stack entry that refers to the register and emits
nothing; a `local.set` or `tee` pops straight into the register, after sending any older reference to it to
memory (`sync_local`), so it keeps the old value. A float operation never writes a register local (it copies
first). The routines for imports use `x19`-`x25` freely, so `wasi_arm64.prologue` saves them below the entry
`sp` and every exit restores them (`leave`); `x27` points below that area, so a routine's scratch space cannot
overwrite it (a first version did, and `tests/test_native_arm64_programs.rs`'s file test caught it). The
`thread-spawn` routine, which returns to translated code, now uses `x0`-`x15` only.

| benchmark | C -O2 | phase 1 | phase 2 | ratio after phase 1 | ratio after phase 2 |
|---|---|---|---|---|---|
| binarytrees 16 | 0.14 s | 0.50 s | 0.39 s | 2.9x | 2.9x |
| fannkuch 10 | 0.12 s | 1.00 s | 0.58 s | 8.2x | 4.8x |
| knucleotide | 0.01 s | 0.05 s | 0.04 s | 7.7x | 6.7x |
| mandelbrot 1000 | 0.03 s | 0.22 s | 0.12 s | 7.1x | 3.9x |
| nbody 1000000 | 0.03 s | 0.44 s | 0.24 s | 13.8x | 7.6x |
| spectralnorm 1000 | 0.03 s | 0.19 s | 0.19 s | 6.9x | 6.8x |
| spigot 10000 | 0.93 s | 2.23 s | 1.57 s | 2.4x | 1.7x |

(C's time varies a little between runs, so the ratios of the unchanged ones differ in the last digit.) The compiler
builds itself on AArch64 in 4.6 s (22 s at the baseline, 6.0 s after phase 1) and is 6.0 MB (7.9 MB before). What
is left is call-bound: `binarytrees` and `spectralnorm` spend their time in calls, whose arguments are still pushed
as 16-byte stack slots; that is phase 3.

## Phase 3: done (2026-10-30)

A wasm function called from wasm takes its first eight arguments in `x0`-`x7` (integers) and `d0`-`d7` (floats),
each class counted separately, and returns in `x0` or `d0`. `plan_conv` decides per function: the ones whose
address escapes (the elements of the table, the start function, `_start`, and a thread's start function) keep the
stack convention, since they are entered from outside translated code; so does a function with more than eight
arguments of a class. A call site spills the entries below its arguments, pops the arguments last first into their
registers, and calls; a register-convention function stores its parameters below the frame (or keeps them in
their register locals) and does not flush at its return. The calls to imports keep the stack convention.

The same phase adds **guards**: an `if` with no result and no `else` whose body ends in `unreachable`, `br`, or
`return` (a bounds check, an assertion) does not flush the virtual stack at its label. The stack is saved at the
`if`, the body runs on a copy, and the fall-through continues with the saved one; a body that falls through anyway
is an error ("native arm64: a guard's body fell through").

| benchmark | C -O2 | phase 2 | phase 3 | ratio after phase 2 | ratio after phase 3 |
|---|---|---|---|---|---|
| binarytrees 16 | 0.14 s | 0.39 s | 0.40 s | 2.9x | 2.9x |
| fannkuch 10 | 0.12 s | 0.58 s | 0.54 s | 4.8x | 4.5x |
| knucleotide | 0.01 s | 0.04 s | 0.04 s | 6.7x | 7.5x |
| mandelbrot 1000 | 0.03 s | 0.12 s | 0.12 s | 3.9x | 3.9x |
| nbody 1000000 | 0.03 s | 0.24 s | 0.21 s | 7.6x | 6.8x |
| spectralnorm 1000 | 0.03 s | 0.19 s | 0.10 s | 6.8x | 3.8x |
| spigot 10000 | 0.93 s | 1.57 s | 1.50 s | 1.7x | 1.6x |

The register convention alone changed nothing measurable (a call's cost is the frame and the saves, not the
arguments); the guards are where `spectralnorm` halved, since its inner loop's bounds checks no longer spill the
stack. Verified as before: every benchmark's output, 19 AArch64 tests, `tools/arm64_bootstrap.sh` (107 programs
identical, 1m34s), and 6,000 generated programs (`tools/arm64_fuzz.py`, seeds 3, 7, 11, 21) with no difference but
the default NaN's sign (the fuzzer also accepts a top-bit difference in an integer a program prints when it
reinterprets floats, which carries that sign).

What is left against C is the instruction selection: a binary operation still moves its operands through
`x0`/`x1`, a constant operand is not an immediate, and a load or store adds `x28` and the address before the access
instead of using `[x28, wN, uxtw]`.

## Phase 4: done (2026-10-30)

Instruction selection, the cost the disassembly of phase 3 showed: a binary operation moved its operands
through `x0` and `x1`, a constant was never an immediate, and a memory access added `x28` and the address
before the access. Now:

- **Operands where they are.** `src` returns the register that holds the top value (a pool register, or a register
  local, read only) or loads it into a scratch register, and the result goes straight into a fresh pool
  register: `add w9, w19, w21`, not `mov x1,..; mov x0,..; add w0,w0,w1; mov x9,x0`. Integer operations,
  comparisons, `select`, `if` and `br_if` conditions (`cbz`/`cbnz` on the register), the extensions, `clz`/`ctz`,
  global access, and the float operations (`src_fp`, a destination register of their own: `fadd d17, d10, d11`)
  all use it. A local's `local.set` of a value the last instruction just computed retargets that instruction
  to the local's register (`add w19, w19, #1`, not an `add` and a `mov`): the instruction's Rd field is
  rewritten, only when nothing was emitted since (`L.def_end`).
- **Immediates.** A constant right operand is an immediate for add and sub (a negative one turns the instruction
  around), comparisons (`cmp`, or `cmn` for a negative), shifts, and, or and xor with a bitmask immediate
  (`arm64.logic_imm_field`: a run of ones, rotated), a multiplication by a power of two (a shift), an
  unsigned division or remainder by a power of two (a shift, a mask). A division by any other nonzero
  constant has no zero check (nor, for a divisor other than -1, the overflow check). Float constants that fit
  8 bits (`fmov d, #2.0`; `arm64.fp_imm8`) and zero are not loaded through `x16`.
- **Addressing.** A load or store with offset 0 is `ldr w, [x28, wN, uxtw]`; with an offset the instruction
  can hold, `add x16, x28, wN, uxtw` then the access with the offset (the add is shared by consecutive accesses
  through the same register); only an offset that does not fit costs a `mov`.
- **Direct conditional branches.** A `br_if` whose target needs no stack adjustment is the conditional branch
  itself (`b.lt target`, `cbnz`), not a branch over an unconditional one; forward ones are patched with a
  mark bit in the fixup list. Their reach is 1 MiB, so a function of 1,500 or more instructions keeps the old form.
- **Constant locals.** A declared local set once, from a constant, with every get after the set and inside the
  block that holds it (an `else` ends its block) is that constant: its gets are constants (immediates) and its
  set does nothing. The `for` loops' step and (literal) end are such locals. A local that is set more than
  once, as the code optimizer's local reuse makes of most of them, is not.
- **More float registers.** Float locals get d8-d15 and d28-d31 (12), the pool v16-v27.

| benchmark | C -O2 | phase 3 | phase 4 | ratio after phase 3 | ratio after phase 4 |
|---|---|---|---|---|---|
| binarytrees 16 | 0.14 s | 0.40 s | 0.26 s | 2.9x | 1.9x |
| fannkuch 10 | 0.12 s | 0.54 s | 0.35 s | 4.5x | 2.9x |
| knucleotide | 0.01 s | 0.04 s | 0.04 s | 7.5x | 5.5x to 6.9x |
| mandelbrot 1000 | 0.03 s | 0.12 s | 0.06 s | 3.9x | 2.0x |
| nbody 1000000 | 0.03 s | 0.21 s | 0.16 s | 6.8x | 4.9x |
| spectralnorm 1000 | 0.03 s | 0.10 s | 0.06 s | 3.8x | 2.0x |
| spigot 10000 | 0.93 s | 1.50 s | 1.12 s | 1.6x | 1.2x |

(knucleotide runs in 0.04 s, so its ratio is mostly start-up.) Two bugs the tests caught on the way: a float
local in `d28`-`d31` read as an integer register by the prologue's `(lt r 19)` test, which zeroed `x28`
(the memory base); and the shared address register, valid only until a label, where another path arrives.
Verified as before: every benchmark's output, the AArch64 tests (a new one runs operations with constant
operands on a spread of values on both backends and compares the output), `tools/arm64_bootstrap.sh`
(107 programs identical), and 8,500 generated programs (`tools/arm64_fuzz.py`, seeds 31 to 33, 41, 42) with no
difference. The encoder gained `add_uxtw`, `and_ri`/`orr_ri`/`eor_ri`, `fmov_imm` (checked against Apple's
assembler like the rest: 15,342 vectors).

What is left against C: the locals past the 7 integer registers live in frame slots (a big function's `sub x16,
x29, #off; ldr` per access); a value that is the result of an `if` or `else` (every `&&` and `||`) goes through
the machine stack; a loop's step and end are not constants when the code optimizer reuses their local.

## Phases, each measured against the table above

1. **The virtual stack, constants, typed registers, fused compare and branch, addressing** (locals still in
   frame slots). Expected: 3 to 5 times faster.
2. **Locals in registers**, with the saves in the import routines. Expected: another 2 to 3 times.
3. **The register calling convention** (and guard-ifs). Expected: 1.2 to 1.5 times on call-heavy programs (binarytrees); measured: only the guards helped.
4. **Instruction selection** (operands in place, immediates, uxtw addressing, direct branches, constant locals). Measured: 1.2 to 2 times.

A phase is done when every benchmark's output is unchanged, `tests/test_native_arm64*.rs` and
`tools/arm64_bootstrap.sh` pass (the compiler built by the optimized backend compiles itself to the same
bytes and every repository program to the Rust command's bytes), the corpus of `tests/aipl` runs
byte-identical to the x86-64 build's, and the table is re-measured.

## Risks

- A wrong register allocation is a wrong program, not a slow one: the bootstrap's fixed point (the
  compiler compiling itself under the optimized code), the 107-program corpus, and
  `tools/arm64_fuzz.py` (the generated programs of `tools/run_fuzz.py` on AArch64 against x86-64: 3,000 cases
  at the end of phase 1, with trap messages compared, and a planted fault in float subtraction fails 41 of
  300; 4,500 more after phase 2) are the net. The one difference it accepts is the sign of a default NaN (a 0/0 is negative on x86-64
  and positive on AArch64; wasm leaves it open).
- The seed (`bootstrap/aipl-linux-aarch64`) contains the baseline backend. Its stage 2 is the optimized
  compiler, which differs from the seed until the seed is refreshed; that is the normal state, and is
  checked by `tools/arm64_bootstrap.sh` without `SEED_FROM_SOURCE`.
- Code size grows with the saves and the spills; the compiler is 7.9 MB now.
