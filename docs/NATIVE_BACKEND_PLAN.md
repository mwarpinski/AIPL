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

**Correctness by comparison.** Every existing test program has a known-good wasm build and runs under `aipl-run`. Each native task is tested by compiling the same programs natively and requiring identical stdout and exit status. This is the same discipline that kept the VM, the Rust backend, and the self-hosted compiler in agreement.

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
- **Linear memory** reserved with `mmap` at start-up: 64 MiB (the 1024-page cap) reserved up front, so it never moves; a base register points at it. Out-of-range accesses must trap like wasm. Decide in NE8 between explicit bounds checks (simple) and a guard region with a signal handler (faster); start explicit.
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

All new code is AIPL, in `aipl_src/native/` (each file created by the task that fills it, never as an empty stub):

| File | Job |
|---|---|
| `wasm_reader.aipl` | decode a wasm module into typed records (sections, functions, instructions) |
| `x64.aipl` | x86-64 instruction encoder: one function per instruction form, byte-exact |
| `lower.aipl` | the baseline compiler: wasm instructions to x86-64 per function, label and branch fix-ups |
| `runtime.aipl` | start-up code, trap handler, and the WASI routines, emitted as machine code |
| `elf.aipl` | ELF64 writer |
| `native.aipl` | the entry point: wasm bytes in, executable bytes out |

The driver gains a native target; `aipl compile --exe` uses it on Linux x86-64 and the launcher everywhere else. The only Rust involved is the CLI flag, until the CLI itself moves to AIPL.

## Tasks (NE1-NE18)

Like the audit's P-tasks: each is small enough for one session, lives on its own branch `features/native_elf/neN` (e.g. `features/native_elf/ne1`) (cut from `development`, merged back when done), and ends with a check that must actually pass. "Done when" is the bar: a task that returns a number without the check passing is not done (PROGRESS.md, "Lessons"). Every task updates PROGRESS.md with what landed and any surprise, and adds its test programs to the shared native test list (NE5) so earlier tasks stay covered.

Order: NE1 → NE2 → NE3 → NE4 → NE5, then NE6-NE9 in order, then NE10-NE14 (NE12-NE14 can go in any order), then NE15 → NE16 → NE17 → NE18.

### Group A: reading wasm

**NE1: wasm_reader, module structure.** Depends on: nothing.
Create `aipl_src/native/wasm_reader.aipl`: decode a wasm module (bytes as `(ptr str.Bytes)`) into typed records: header check, type section (function signatures), imports (module, name, kind, type index; memory imports with limits and the shared flag), function section, table, memory, global (type, mutability, init), exports, start, element (active, function indices), data count, and data segments (active with offset, or passive). Code bodies are kept as byte ranges for NE2. Use generic `std/vec` collections. Unknown sections or encodings our compiler never emits are a clear error naming them.
Done when: a Rust test decodes the wasm of every repository program plus the thread and generics test programs and checks each section's counts and contents against `wasmparser`.

**NE2: wasm_reader, function bodies.** Depends on: NE1.
Decode each body: local declarations, then the instruction stream into a vec of instruction records (opcode, immediates: indices, block types, memargs, constants including LEB128 i32/i64 and f64 bits), including the `0xFC` (`memory.init`) and `0xFE` (atomics) prefixes. Exactly the list in "Input" above; anything else is an error naming the opcode.
Done when: the same Rust test also compares every function's instruction count and opcode sequence with `wasmparser`'s operator reader.

### Group B: writing machine code

**NE3: x64 encoder.** Depends on: nothing (can run alongside NE1-NE2).
Create `aipl_src/native/x64.aipl`: an append-only code buffer and one function per instruction form the lowering needs: register-to-register and immediate `mov`/`add`/`sub`/`and`/`or`/`xor`/`cmp`/`test`, shifts, `imul`, `div`/`idiv` with `cdq`/`cqo`, `movzx`/`movsx`, loads and stores with base + displacement (and base + index), `push`/`pop`, `lea`, `setcc`, `jmp`/`jcc`/`call` with 32-bit relative displacements and a fix-up helper for forward jumps, `call` through a register, `ret`, `syscall`, `ud2`, and the `lock` prefix; 32- and 64-bit operand sizes, with REX handling for r8-r15. SSE2 forms are added in NE13.
Done when: a byte-level test (an AIPL self-test, wired into `test_suite.aipl`) checks each form against known encodings, including every register in both halves of the register file. Test vectors come from the Intel manual or `objdump` output pasted into the test, not from the encoder itself.

**NE4: ELF writer and a first executable.** Depends on: NE3.
Create `aipl_src/native/elf.aipl` (an ELF64 executable header and program headers for a read+execute code segment and a read+write data segment, entry point, page-aligned layout) and a tiny `runtime.aipl` start-up stub. Build, from AIPL, an executable that writes "hello from native AIPL" with the `write` system call and exits with status 7 through `exit_group`.
Done when: a Rust test runs the generated file and checks stdout and exit status 7, and `readelf -h` (if installed, not required by the test) shows a valid header.

**NE5: lowering skeleton and the native test harness.** Depends on: NE2, NE4.
Create `aipl_src/native/lower.aipl` and `native.aipl` (wasm bytes in, executable bytes out). Baseline design: each function gets a frame (`rbp`-based) with its locals; the wasm value stack lives on the machine stack. Implement `i32.const`, `local.get/set/tee`, `drop`, `call` (direct), `return`, `end`, and the start-up path: `_start` calls the module's `_start` export and exits 0. Add a Rust helper `assert_native_matches(program)` that builds the program as wasm (run under `aipl-run`) and natively, and requires identical stdout, stderr, and exit status; it keeps one shared list of programs that later tasks extend.
Done when: programs whose `main` only moves constants between locals and calls functions match, and the harness exists with that list.

### Group C: the integer core

**NE6: i32 arithmetic and traps.** Depends on: NE5.
All `i32` arithmetic, bitwise, shift (count masked to 5 bits, as wasm), and comparison instructions, `eqz`, `i32.wrap_i64`. Division and remainder trap on zero and `INT_MIN / -1` exactly where wasm does (rem of `INT_MIN % -1` is 0, not a trap). The trap path: write `"wasm trap: <reason>"` to stderr and exit 134, matching `aipl-run`.
Done when: the i32 cases of `tests/test_differential.rs` (wrapping, shifts, division) pass through the native harness, including trap cases.

**NE7: control flow.** Depends on: NE6.
`block`, `loop`, `if`/`else`, `br`, `br_if`, `unreachable`, with a label stack and branch fix-ups; block results; branches that leave values in the right place.
Done when: `examples/math_core.aipl` and the `tests/test_control_flow.rs` programs (break, continue, return in loops, short-circuit `and`/`or`) match natively.

**NE8: linear memory.** Depends on: NE7.
At start-up `mmap` the 64 MiB maximum (reserve, then make the first 16 pages usable), keep its base in a dedicated register, and copy active data segments in. Loads and stores for every width with explicit bounds checks that trap like wasm; `memory.size`; `memory.grow` (up to 1024 pages, -1 beyond).
Done when: the memory-layout and struct/array programs (`tests/test_memory_layout.rs`, `p8_structs_and_arrays` in `test_differential.rs`) match natively, including traps on out-of-range access.

**NE9: single-threaded atomics.** Depends on: NE8.
`i32.atomic.load/store`, `rmw.add/xchg/cmpxchg` as `lock` instructions; alignment traps as wasm. This is what every allocation uses.
Done when: `examples/quicksort.aipl`, `examples/accounts.aipl`, and the atomics program in `tests/test_threads.rs` (`atomics_agree_without_threads`) match natively.

### Group D: the runtime surface

**NE10: WASI part 1.** Depends on: NE9.
Start-up captures argc, argv, and envp from the initial stack. Implement `fd_write`, `fd_read`, `fd_close`, `proc_exit`, `args_sizes_get`, `args_get`, `environ_sizes_get`, `environ_get`, `clock_time_get`, `random_get` as routines in `runtime.aipl`, in the WASI layout and errno convention.
Done when: the args/env, stdin, clock, and random programs from `tests/test_wasi.rs` and `tests/test_runner.rs` match natively (clock and random checked by property, as those tests do).

**NE11: WASI part 2, files and the access rules.** Depends on: NE10.
`path_open` and `path_unlink_file` through `openat`/`unlinkat`, with fd 3 = working directory and fd 4 = `/`, and the decided file-access rules: absolute paths allowed by default and refused with `--sandbox`; no path escapes its directory with `..`. The sandbox flag reaches the executable from the compiler (a byte in the data segment).
Done when: `word_count`, `word_freq`, the file programs in `test_wasi.rs`, and the `--sandbox` cases in `test_runner.rs` match natively.

### Group E: the rest of the instruction set

**NE12: i64.** Depends on: NE9.
All `i64` arithmetic, comparisons, `i64.extend_i32_s/u`, i64 loads and stores, with the same trap rules as NE6.
Done when: `tests/test_i64.rs` programs match natively.

**NE13: floats.** Depends on: NE9.
SSE2 encoder forms in `x64.aipl`, then `f32`/`f64` constants, arithmetic, comparisons (NaN compares false, as wasm), loads/stores, `f64.convert_i64_s`, `i64.trunc_f64_s` (traps on NaN and out of range, exactly at wasm's bounds), and the reinterprets.
Done when: the float programs in `test_differential.rs`, `examples/matrix_mult.aipl`, and the float-literal cases match natively.

**NE14: function references.** Depends on: NE9.
The function table, element section, and `call_indirect` with its type check (trap on mismatch, as wasm).
Done when: `tests/test_refs.rs` programs and `vec.sort_by` users (`word_freq`, `std/vec` tests) match natively.

### Group F: threads

**NE15: thread creation.** Depends on: NE11, NE14.
Threaded modules: shared memory, passive data plus the start function's `memory.init`, the per-thread global, and `thread-spawn` as `clone` with an `mmap`ed stack that runs `wasi_thread_start(tid, record)`. A thread that traps or exits ends the process, as under `aipl-run`.
Done when: `join_returns_the_worker_result` and `concurrent_allocations_never_overlap` from `test_threads.rs` match natively.

**NE16: waiting and locks.** Depends on: NE15.
`memory.atomic.wait32` and `notify` as `futex` wait/wake; the lock and join paths built on them.
Done when: every `tests/test_threads.rs` program matches natively, repeatedly (run each 20 times to shake out races).

### Group G: finishing

**NE17: the compiler, natively.** Depends on: NE16 and NE12-NE13.
Build `aipl_src/driver.aipl` natively.
Done when: the native `aiplc` compiles every repository program to the same wasm as the Rust toolchain, compiles itself to identical wasm, and building it natively twice gives identical executables (reproducible). Record its size and speed against the launcher build in PROGRESS.md.

**NE18: make it the default.** Depends on: NE17.
`aipl compile --exe` produces a native executable on Linux x86-64 (and the launcher bundle elsewhere, or with `--target wasm`); `--sandbox` works for both. Update AIPL_SPEC.md (a native backend section beside 6.5), README, PROGRESS, LANGUAGE_GAPS, and the audit.
Done when: the full test suite passes, `test_runner.rs` covers both kinds of executable, and the docs describe the native path.

## Testing

- A Rust test helper (NE5) compiles a program as wasm under `aipl-run` and natively, and requires identical stdout, stderr, and exit status. Every task adds its programs to one list, so earlier tasks stay covered.
- During development, `objdump -d` (if installed) is a useful aid for checking encodings, but tests must not depend on it.
- Instruction encodings get small unit tests of their own (known instruction → known bytes), since one wrong byte in an encoder is otherwise hard to find.

## Risks and how they are handled

- **Encoding mistakes** are the most likely bug class: covered by the per-instruction byte tests and by whole-program comparison.
- **Floating-point corner cases** (NaN, the trapping `i64.trunc_f64_s` range): follow the wasm spec exactly; the existing float differential tests cover them.
- **Threads** are the hardest part; they come last (NE15-NE16), when everything else is solid.
- **Scope:** this is several thousand lines of AIPL. The tasks are ordered so each is useful and testable on its own; stopping after any of them leaves a working toolchain.
