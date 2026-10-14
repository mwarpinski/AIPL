# Faster native code

**Status: approved 2026-10-14, in progress: F0, F1, F2, and F4 done (docs/BENCHMARKS.md, "The value-tracking translator"); F3 next.** Every decision at the
end was taken as recommended.

## The gap

Native executables are slower than the same program as wasm under
wasmtime, 1.6x to 5.8x (docs/BENCHMARKS.md; the last full measurement,
before the null checks):

| Benchmark | native | wasm (wasmtime) | native / wasm |
|---|---|---|---|
| spigot | 2.479 s | 1.589 s | 1.6x |
| fannkuch | 1.019 s | 0.251 s | 4.1x |
| nbody | 0.697 s | 0.120 s | 5.8x |
| knucleotide | 1.141 s | 0.734 s | 1.6x |
| pidigits | 14.213 s | 2.650 s | 5.4x |

A native executable should not be the slow way to run AIPL. The two
measured causes (BENCHMARKS.md, "Locals in registers"):

1. **One instruction at a time.** `lower.aipl` translates each wasm
   instruction on its own, moving every operand through rax and the
   machine stack: `n - 1` is `push rax; mov eax,1; mov rcx,rax; pop rax;
   sub eax,ecx`, where a compiler emits `lea eax,[rbx-1]` or `dec`. The
   loop `s = s + a[i]` is about 60 instructions; an optimising compiler
   emits under 10. Constants as immediates and two values in registers
   were each tried alone and gained nothing while every other value
   still went through memory.
2. **A bounds check on every load and store**, which loads the memory's
   size from a shared cell and compares: 6-14% when removed (measured
   unsafely, as an experiment).

## The design

**A single-pass translator that tracks where each value is**, the design
of V8's Liftoff and of wasmtime's Winch: still one pass, no intermediate
representation and no optimiser, so the translator stays small, fast, and
reviewable, and stays in AIPL.

- **An abstract value stack.** For each value on the wasm stack the
  translator records where it is: a constant, a local, a register, or a
  stack slot. `i32.const 1` and `local.get` emit nothing; they push a
  record. Code is emitted when an instruction consumes its operands, and
  it uses them where they are: `local.get $n; i32.const 1; i32.sub` becomes
  `lea eax, [rbx - 1]` with `$n` in rbx, one instruction instead of five.
- **A pool of scratch registers.** Results go to a free register (rax,
  rcx, rdx, rsi, rdi, r8-r11; xmm0-xmm15 for floats); when none is free
  the oldest value on the stack is spilled to its slot.
- **Fixed states where control flow meets.** At a block's end, a loop's
  start, and a branch, every value is moved to an agreed place (its stack
  slot; a block's result in rax or xmm0), as the current translator does
  for everything. Straight-line code, where the time goes, keeps values
  in registers.
- **Comparisons fused with branches.** `i32.lt_s; br_if` becomes `cmp;
  jl` instead of `setl; movzx; test; jnz`.
- **Memory operands.** A load's address and an operand already in memory
  are used in place (`add eax, [r15 + rcx + 8]`).
- **Arguments in registers.** The first six integer arguments in rdi, rsi,
  rdx, rcx, r8, r9 and float arguments in xmm0-xmm7, as the System V
  convention does, instead of pushed. Results in rax / xmm0, and several
  results (for value structs, docs/design/VALUES_PLAN.md) in rax, rdx and
  then a per-thread return area.
- **More locals in registers.** All of a function's locals that fit in the
  callee-saved registers (rbx, r12-r14), chosen by use as now; scratch
  registers in a function that calls nothing.

**Memory checks by guard pages**, as wasmtime does. The memory is already
reserved past its current size with no access (memory.grow opens pages);
reserving 8 GiB covers every 32-bit address plus every offset, so an
out-of-bounds access always lands in no-access memory and the hardware
stops it. A `SIGSEGV` handler checks that the fault is inside the
reservation and prints the same trap message and call chain the explicit
check prints today; any other fault is a crash, as now. Loads and stores
lose their compare and branch, and no longer read the shared size cell.

**What does not change:** the executables' behaviour, byte for byte in
their output and exit status (test_native compares every program with
aipl-run); the wasm reader, the encoder, the ELF writer, and the WASI
routines (except where they take arguments); one pass, in AIPL.

## How it is kept correct

The translator is changed in place, a stage at a time, and each stage
must pass before the next starts (the plan first had a second translator
beside the old one; test_native already compares every native program
with aipl-run, a stronger reference than the old translator):

- every test in test_native, test_benchmarks, test_native_elf, and the
  doc examples compiled natively;
- `tools/run_fuzz.py` (generated programs run in the VM, as wasm, and
  natively, and compared), for a long run before the switch, with its
  generator extended to stress what the rewrite changes: deep expressions
  (spills), many locals and arguments, branches inside expressions;
- the encoder's instructions against GNU as (tools/x64_vectors.py) for
  every new form (memory operands, `lea`, fused compares).

## Stages

Each stage is measured on all eight benchmarks (best of five) and recorded
in docs/BENCHMARKS.md; a stage that does not pay for itself is reverted.

| Stage | What | Expected |
|---|---|---|
| F0 | Re-measure the baseline (null checks included), and the per-benchmark instruction counts of the hot loops | a table to beat |
| F1 | The abstract value stack, scratch registers, fixed states at control flow; arguments still pushed | most of the gain: fewer instructions on every loop |
| F2 | Fused compare-and-branch, memory operands, immediates, `lea` | loops with comparisons and array reads |
| F3 | Arguments and results in registers; locals in registers widened; several results (needed by value structs) | call-heavy code: pidigits, binarytrees, spigot |
| F4 | Guard pages and the fault handler; the explicit check removed | 6-14% on memory-bound loops |
| F5 | Update the spec (6.6), BENCHMARKS.md, and the README's numbers | |

Target: native within 2x of wasmtime on every benchmark (from 5.8x at
worst). A single-pass compiler's code is typically 1.5-2x slower than an
optimising compiler's such as wasmtime's Cranelift (V8's Liftoff against
its TurboFan, wasmtime's own Winch against Cranelift), so matching
wasmtime outright needs the optimising tier below, a separate decision
once this is measured. Short runs gain more: wasmtime compiles at start-up,
a native executable does not.

## Alternatives considered

- **An optimising tier** (an intermediate representation, register
  allocation across blocks, inlining), as Cranelift and LLVM do. It is the
  only way to match wasmtime outright, and it is several times the size
  of the whole native backend. Better decided after F1-F4 show how much
  of the gap remains; the single-pass translator would stay as its
  first tier, as Liftoff does under TurboFan.
- **Small fixes to the current translator.** Tried (constants as
  immediates, two cached values); they measured nothing, because the cost
  is that every value goes through memory, which only a rewrite of how
  values are tracked changes.
- **Keeping the explicit memory check.** Simpler (no signal handler), at
  6-14% on memory-bound code. Guard pages are how every fast wasm runtime
  does it.

## Decisions (taken as recommended, 2026-10-14)

1. **A single-pass, Liftoff-style rewrite in AIPL** (recommended), or an
   optimising tier.
2. **Guard pages with a fault handler** (recommended, stage F4), or keep
   the explicit check.
3. **The internal calling convention moves to registers** (recommended,
   stage F3; internal only, so nothing outside the executable sees it).
4. **Target: within 2x of wasmtime on every benchmark** (recommended;
   matching it needs an optimising tier, decided later), or a different
   bar.
