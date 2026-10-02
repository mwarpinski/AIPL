# Linux x86-64 native backend: implementation plan

Written 2026-10-02, before any native code exists. This is the plan for step 6 of PROGRESS.md ("Next steps"). Read PROGRESS.md's "Direction" section first; the decisions below follow from it.

## Goal

`aipl compile --exe prog.aipl -o prog` on Linux x86-64 produces a small, standalone ELF executable written entirely by AIPL: no wasmtime, no launcher, no Rust in the path. It must behave exactly like the same program run as wasm under `aipl-run`: same output, same exit status, same file-access rules, same thread semantics. Every other platform keeps using the launcher.

## Approach

**Translate wasm, not AIPL.** The pipeline is

```
prog.aipl --(resolver, generics, codegen: existing AIPL)--> prog.wasm --(new, AIPL)--> prog (ELF)
```

so new language features only ever change AIPL → wasm, and the native backend has a fixed, small input: the wasm our own compiler emits. It does not need to handle arbitrary wasm.

**A baseline compiler.** One pass per function, like V8's Liftoff or wasmtime's Winch: walk the wasm instructions in order and emit machine code for each, keeping the wasm value stack on the machine stack, with values loaded into a few scratch registers per instruction. No register allocator and no optimisation at first. Expected speed: perhaps 2-5x slower than wasmtime's optimising Cranelift output, which is still fast; output is a few KB to a few hundred KB instead of 18 MB. Optimisation can come later without changing anything around it.

**Correctness by comparison.** Every existing test program has a known-good wasm build and runs under `aipl-run`. Each native milestone is tested by compiling the same programs natively and requiring identical stdout and exit status. This is the same discipline that kept the VM, the Rust backend, and the self-hosted compiler in agreement.

## Input: what the backend must translate

Exactly what `src/compiler/wasm.rs` and `aipl_src/codegen.aipl` emit (they are byte-identical):

- **Sections:** type, import, function, table, memory (or an imported shared memory), global, export, start, element, data count, code, data (active, or passive in threaded modules).
- **Control:** `block`, `loop`, `if`/`else`, `br`, `br_if`, `return`, `call`, `call_indirect`, `unreachable`, `end`, `drop`.
- **Variables:** `local.get/set/tee`, `global.get/set` (one global, threaded modules only).
- **Constants and arithmetic:** `i32.const`, `i64.const`, `f64.const`, and the integer and float arithmetic, bitwise, shift, and comparison families for `i32`/`i64`/`f32`/`f64` (chosen by type in `arith_instruction` / `compare_instruction`).
- **Conversions:** `i32.wrap_i64`, `i64.extend_i32_s/u`, `f64.convert_i64_s`, `i64.trunc_f64_s` (trapping), `i64/f64.reinterpret`.
- **Memory:** `i32/i64/f32/f64.load/store`, `i32.load8_u`, `i32.store8`, `memory.size`, `memory.grow`, `memory.init`.
- **Atomics:** `i32.atomic.load/store`, `i32.atomic.rmw.add/xchg/cmpxchg`, `memory.atomic.wait32/notify`.
- **Imports (the runtime surface):** WASI `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, `path_unlink_file`, `args_sizes_get`, `args_get`, `environ_sizes_get`, `environ_get`, `clock_time_get`, `random_get`, and wasi-threads `thread-spawn`. Nothing else.

If the compiler ever emits an instruction outside this list, the native backend must fail with a clear error naming it (no silent fallbacks, audit B1).

## Output: what the executable contains

- **An ELF64 file** for x86-64 Linux, statically linked, no libc: a header, a code segment (read + execute), and a data segment (read + write). Entry point: compiled `_start`.
- **System calls directly** (`syscall` instruction), as Go does on Linux: `read`, `write`, `openat`, `close`, `unlinkat`, `exit_group`, `mmap`, `clock_gettime`, `getrandom`, `clone`, `futex`.
- **Linear memory** reserved with `mmap` at start-up: 64 MiB (the 1024-page cap) reserved up front, so it never moves; a base register points at it. Out-of-range accesses must trap like wasm. Decide in M3 between explicit bounds checks (simple) and a guard region with a signal handler (faster); start explicit.
- **Traps** (unreachable, divide by zero, out-of-range memory, bad conversions) print a message to stderr and exit with status 134, as `aipl-run` does.
- **Program start-up:** argc/argv/envp read from the initial stack (for the `args_*`/`environ_*` imports), preopened directory 3 = the working directory and 4 = `/`, then run the module's start function (threaded modules) and `_start`.

## The runtime (WASI) layer

Each import becomes a small routine emitted into every executable that uses it, written as machine code by the backend (in AIPL), not linked from anywhere:

- `fd_*`, `path_open`, `path_unlink_file`: translate to `read`/`write`/`openat`/`close`/`unlinkat`, mapping Linux errors to the WASI errno convention our lowerings expect (any error becomes -1 in AIPL anyway).
- **File-access rules (decided 2026-10-02):** the same as wasm. Relative paths resolve in the working directory; absolute paths are allowed by default and refused with `--sandbox`; no path may escape the granted directory with `..`. The check happens before every open or unlink.
- `clock_time_get`: `clock_gettime(CLOCK_REALTIME | CLOCK_MONOTONIC)`. `random_get`: `getrandom`.
- `args_*`/`environ_*`: copy from the initial stack in WASI layout.

## Threads

- `thread-spawn` becomes `clone` with a freshly `mmap`ed stack for the new thread, which runs the module's `wasi_thread_start(tid, record)`.
- The per-thread global (the runtime scratch pointer) lives in a per-thread register or a thread-local slot.
- Atomics are x86 `lock` instructions (`lock xadd`, `xchg`, `lock cmpxchg`); `memory.atomic.wait32/notify` are `futex` wait/wake.
- The required semantics are already pinned down by `tests/test_threads.rs` (counter, join, mutex, concurrent allocation, concurrent printing) and must pass natively.

## Where the code goes

All new code is AIPL, in `aipl_src/native/` (created milestone by milestone, never as empty stubs):

| File | Job |
|---|---|
| `wasm_reader.aipl` | decode a wasm module into typed records (sections, functions, instructions) |
| `x64.aipl` | x86-64 instruction encoder: one function per instruction form, byte-exact |
| `lower.aipl` | the baseline compiler: wasm instructions to x86-64 per function, label and branch fix-ups |
| `runtime.aipl` | start-up code, trap handler, and the WASI routines, emitted as machine code |
| `elf.aipl` | ELF64 writer |
| `native.aipl` | the entry point: wasm bytes in, executable bytes out |

The driver gains a native target; `aipl compile --exe` uses it on Linux x86-64 and the launcher everywhere else. The only Rust involved is the CLI flag, until the CLI itself moves to AIPL.

## Milestones

Each one is a separate session or two, ends with tests passing, and leaves the tree usable.

| # | Milestone | Done when |
|---|---|---|
| M0 | `wasm_reader.aipl`: decode our modules | it decodes every repository program's wasm and reports matching section, function, and instruction counts (checked against `wasmparser` in a Rust test) |
| M1 | `elf.aipl` + `x64.aipl` basics: a hand-built "hello" ELF | an AIPL-written executable prints a line via `write` and exits with a chosen status |
| M2 | integer core: locals, i32 arithmetic and comparisons, control flow, direct calls, traps | `examples/math_core.aipl` functions give the same results natively as under wasmtime |
| M3 | linear memory: reservation, loads/stores, store guard, allocation, `memory.grow/size`, data segments | `quicksort`, `accounts`, and the struct/array differential programs agree |
| M4 | the WASI layer with the file-access rules | `word_count`, `word_freq`, the `test_wasi` and `test_runner` programs agree, including `--sandbox` refusing absolute paths |
| M5 | i64, f64, conversions, `call_indirect` | `matrix_mult`, the i64/float differential tests, function references agree |
| M6 | threads: `clone`, futex, atomics | every `tests/test_threads.rs` program agrees |
| M7 | self-hosting | the AIPL compiler (`driver.aipl`) built natively compiles itself to identical wasm, and its native build is reproducible |
| M8 | CLI and docs | `aipl compile --exe` is native by default on Linux x86-64; spec and PROGRESS updated |

## Testing

- A Rust test helper compiles a program three ways (wasm under `aipl-run`, native, and where useful the VM) and requires identical stdout, stderr, and exit status. Every milestone adds its programs to one list, so earlier milestones stay covered.
- During development, `objdump -d` (if installed) is a useful aid for checking encodings, but tests must not depend on it.
- Instruction encodings get small unit tests of their own (known instruction → known bytes), since one wrong byte in an encoder is otherwise hard to find.

## Risks and how they are handled

- **Encoding mistakes** are the most likely bug class: covered by the per-instruction byte tests and by whole-program comparison.
- **Floating-point corner cases** (NaN, the trapping `i64.trunc_f64_s` range): follow the wasm spec exactly; the existing float differential tests cover them.
- **Threads** are the hardest part; they come last (M6), when everything else is solid.
- **Scope:** this is several thousand lines of AIPL. The milestones are ordered so each is useful and testable on its own; stopping after any of them leaves a working toolchain.
