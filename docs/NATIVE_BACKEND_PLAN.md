# Linux x86-64 native backend: implementation plan

**Status: done (NE1–NE18, 2026-10-04).** Kept as the design record; where it differs from the code, the code and AIPL_SPEC.md 6.6 win (the memory cap, for one, is now 32768 pages, 2 GiB, not 64 MiB).

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
- **Constants:** `i32.const`, `i64.const`, `f64.const` (no `f32.const`).
- **Integer arithmetic** (`i32` and `i64`): `add sub mul div_s div_u rem_s rem_u and or xor shl shr_s shr_u`.
- **Float arithmetic** (`f32` and `f64`): `add sub mul div`.
- **Comparisons:** `i32`: `eqz eq ne lt_s lt_u gt_s gt_u le_s ge_s`; `i64`: `eq ne lt_s gt_s le_s ge_s`; `f32`/`f64`: `eq ne lt gt le ge`. (Chosen by type in `arith_instruction` / `compare_instruction`; `lt_u` comes from the write-address check, `gt_u` from heap growth.) `wasm_reader`'s `plain_kind` is this list in code, and `tests/test_native_reader.rs` checks that it accepts exactly these.
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
**Done (2026-10-02).** Bodies decode to `Body [locals code]` with `Instr [op a b c offset]` records (prefixed opcodes as `0xFC00`/`0xFE00` + sub-opcode). The test compares every instruction's offset, opcode and immediates (not just the opcode sequence) on every program, on a hand-built body holding every accepted instruction, and checks that every other one-byte opcode and `0xFC`/`0xFE` sub-opcode is rejected with an error naming it.

### Group B: writing machine code

**NE3: x64 encoder.** Depends on: nothing (can run alongside NE1-NE2).
Create `aipl_src/native/x64.aipl`: an append-only code buffer and one function per instruction form the lowering needs: register-to-register and immediate `mov`/`add`/`sub`/`and`/`or`/`xor`/`cmp`/`test`, shifts, `imul`, `div`/`idiv` with `cdq`/`cqo`, `movzx`/`movsx`, loads and stores with base + displacement (and base + index), `push`/`pop`, `lea`, `setcc`, `jmp`/`jcc`/`call` with 32-bit relative displacements and a fix-up helper for forward jumps, `call` through a register, `ret`, `syscall`, `ud2`, and the `lock` prefix; 32- and 64-bit operand sizes, with REX handling for r8-r15. SSE2 forms are added in NE13.
Done when: a byte-level test (an AIPL self-test, wired into `test_suite.aipl`) checks each form against known encodings, including every register in both halves of the register file. Test vectors come from the Intel manual or `objdump` output pasted into the test, not from the encoder itself.
**Done (2026-10-02).** `aipl_src/native/x64.aipl`: forms take `sz` 32/64, registers 0-15 in hardware order, memory operands as `Mem [base index scale disp]` (shortest displacement; SIB for rsp/r12 bases, a displacement for rbp/r13). Beyond the list: `xadd`/`xchg`/`cmpxchg` on memory for the atomics (NE14), and the short `eax` immediate forms, so output matches GNU as byte for byte. Six self-test groups compare about 9 KB of encodings with GNU as output (`tools/x64_vectors.py` regenerates the vectors); `tests/test_native_x64.rs` runs them in the VM and compiled to wasm. Mutation-checked: 14 deliberate encoder bugs, all caught.

**NE4: ELF writer and a first executable.** Depends on: NE3.
Create `aipl_src/native/elf.aipl` (an ELF64 executable header and program headers for a read+execute code segment and a read+write data segment, entry point, page-aligned layout) and a tiny `runtime.aipl` start-up stub. Build, from AIPL, an executable that writes "hello from native AIPL" with the `write` system call and exits with status 7 through `exit_group`.
Done when: a Rust test runs the generated file and checks stdout and exit status 7, and `readelf -h` (if installed, not required by the test) shows a valid header.
**Done (2026-10-02).** Layout: data first at a fixed address (file 0x1000, loaded at 0x401000, read+write, then any zero-filled `bss`), code on the next page (read+execute), and a `PT_GNU_STACK` header so the stack is never executable. Data before code means generated code can use data addresses as 32-bit constants before it knows its own length (no relocations). The hello executable is 8228 bytes. `tests/test_native_elf.rs` runs it, checks that the VM and compiled wasm build identical bytes, and checks every program header with `readelf` when installed; `elf.run_elf_tests` checks the layout and header bytes. Open point for NE18: WASI cannot set file permissions, so a toolchain running as wasm writes the executable without its execute bit; whoever writes the file (today the Rust CLI, later `aipl-run` for the driver) must mark it executable.

**NE5: lowering skeleton and the native test harness.** Depends on: NE2, NE4.
The driver reaches the backend with `(import native/native)` (subdirectory imports, AIPL_SPEC.md 11, added after NE1).
Create `aipl_src/native/lower.aipl` and `native.aipl` (wasm bytes in, executable bytes out). Baseline design: each function gets a frame (`rbp`-based) with its locals; the wasm value stack lives on the machine stack. Implement `i32.const`, `local.get/set/tee`, `drop`, `call` (direct), `return`, `end`, and the start-up path: `_start` calls the module's `_start` export and exits 0. Add a Rust helper `assert_native_matches(program)` that builds the program as wasm (run under `aipl-run`) and natively, and requires identical stdout, stderr, and exit status; it keeps one shared list of programs that later tasks extend.
Done when: programs whose `main` only moves constants between locals and calls functions match, and the harness exists with that list.
**Done (2026-10-03).** `native.aipl` (start-up stub, import stubs, functions, call fix-ups, ELF) and `lower.aipl` (one function: frame, zeroed locals, the instructions above plus `i64.const`). Calling convention: the caller pushes arguments and pops them after the call; the result comes back in rax; every wasm value is one 8-byte stack slot. The `proc_exit` import was needed too, since without it a program's only observable result is exit status 0. Anything else (instructions, imports, a start function) is an error naming it. `tests/test_native.rs` holds the harness: `PROGRAMS` (AIPL source) and `wasm_programs()` (hand-built wasm for cases AIPL cannot express, such as reading a local before writing it), each run under `aipl-run` and natively with identical stdout, stderr, and status. Its first run found a real difference: wasmtime rejects exit statuses outside 0..125 (a trap, status 134), so the native `proc_exit` does too. Mutation-checked with 14 deliberate translator bugs; four first slipped through because every value passed through the same register, and the hand-built programs now catch them.

### Group C: the integer core

**NE6: i32 arithmetic and traps.** Depends on: NE5.
All `i32` arithmetic, bitwise, shift (count masked to 5 bits, as wasm), and comparison instructions, `eqz`, `i32.wrap_i64`. Division and remainder trap on zero and `INT_MIN / -1` exactly where wasm does (rem of `INT_MIN % -1` is 0, not a trap). The trap path: write `"wasm trap: <reason>"` to stderr and exit 134, matching `aipl-run`. (NE5 left the exact trap text open; it is decided below.)
Done when: the i32 cases of `tests/test_differential.rs` (wrapping, shifts, division) pass through the native harness, including trap cases.
**Done (2026-10-03).** Trap text (decided with the project owner): one line, `<argv[0]>: <reason>` (`./prog: wasm trap: integer divide by zero`), exit 134, printed identically by `aipl-run` (full wasmtime report with `AIPL_BACKTRACE=1`) and by native code, whose start-up saves argv[0] for the shared trap routine (`runtime.emit_trap_routine`). The harness runs both builds under the same path and now compares trap output exactly. Every i32 operator is run on all pairs of 18 edge values in hand-built wasm programs (results checked against Rust's and hashed into the exit status), plus the `test_differential.rs` cases as AIPL source and every trap. Division: `div_s` traps on zero and `INT_MIN / -1`; `rem_s` by -1 skips `idiv` (which would fault on `INT_MIN % -1`) and gives 0. `i32.wrap_i64` emits nothing: i32 operations never read a slot's high half, so NE12's `i64.extend_i32_u/_s` must set it. Mutation-checked (12 deliberate bugs).

**NE7: control flow.** Depends on: NE6.
`block`, `loop`, `if`/`else`, `br`, `br_if`, `unreachable`, with a label stack and branch fix-ups; block results; branches that leave values in the right place.
Done when: `examples/math_core.aipl` and the `tests/test_control_flow.rs` programs (break, continue, return in loops, short-circuit `and`/`or`) match natively.
**Done (2026-10-03).** The translator tracks the value stack's depth at every instruction and keeps a label stack (function body, block, loop, if): a branch resets rsp to its label's entry depth (keeping a carried result), then jumps back to a loop's start or forward to a block's end, patched when the block closes. Code after `br`/`return`/`unreachable` is dead until a reached label ends, and nothing is emitted for it. `unreachable` traps with wasmtime's text. The harness gained `assert_functions_match`: wasmtime computes each function's result, and a generated `main` calls them natively and exits with the number that differ (exact, unlike hashing into an exit status). It runs `math_core` and the control-flow and short-circuit programs (read verbatim from `test_control_flow.rs`); their functions that need memory (results, `mem.*`: `first_non_digit`, `uses_void_return`, `side_effects`, `jump_in_operand`) move to NE8. Hand-built wasm covers what AIPL rarely emits: branches carrying values out of nested blocks over extra stack values, `br_if` taken and not, `if` without `else`, a branch to the function label, `return` from inside a loop, junk in dead code, the `unreachable` trap, and values below a block that must survive a branch out of it (with and without locals; the first mutation round showed returning right after a block hid a missing stack reset). Every harness run now has a 30-second limit: a mutation that miscompiled a loop's exit hung the suite instead of failing it. Mutation-checked: 10 of 11 deliberate bugs caught; the eleventh (not resetting the tracked depth at `else`) only ever overcounts the depth, which costs an unneeded but correct stack reset, so it is not observable.

**NE8: linear memory.** Depends on: NE7.
At start-up `mmap` the 64 MiB maximum (reserve, then make the first 16 pages usable), keep its base in a dedicated register, and copy active data segments in. Loads and stores for every width with explicit bounds checks that trap like wasm; `memory.size`; `memory.grow` (up to 1024 pages, -1 beyond).
Done when: the memory-layout and struct/array programs (`tests/test_memory_layout.rs`, `p8_structs_and_arrays` in `test_differential.rs`) match natively, including traps on out-of-range access.
**Done (2026-10-04), with its program list moved to NE9.** Every AIPL allocation (`mem.alloc`, `new`, `arr.new`, results) is an atomic add on the heap cursor, so the struct/array and memory-layout programs (and the four NE7 functions that need memory) need NE9 and are listed there. NE8 is tested without allocation: string data, the heap cursor's initial value, stores and loads, growing to the 1024-page cap and past it, the top of a fully grown memory, and out-of-bounds loads and stores (AIPL source); every width, offset immediates, an access straddling the end, address + offset not wrapping, an address with leftover high bits, the maximum, fresh pages reading zero, and f32/f64 bits (hand-built wasm). Design: start-up reserves the maximum with `mmap` (no access, so only address space), makes the minimum usable with `mprotect`, copies active data segments (`rep movsb`, added to x64.aipl) and keeps the base in r15; the current size lives in a data cell rather than a register so threads (NE15) share it; every access zero-extends the address and checks address + offset + width against that size; `memory.grow` makes more pages usable, -1 past the maximum. `f64.const` came along (a constant's bits); float arithmetic is still NE13. Mutation-checked: 12 deliberate bugs, all caught after adding two programs the first round showed were missing (an address in bounds only without its offset, and a 4-byte store's neighbours).

**NE9: single-threaded atomics.** Depends on: NE8.
`i32.atomic.load/store`, `rmw.add/xchg/cmpxchg` as `lock` instructions; alignment traps as wasm. This is what every allocation uses.
Done when: `examples/quicksort.aipl`, `examples/accounts.aipl`, and the atomics program in `tests/test_threads.rs` (`atomics_agree_without_threads`) match natively. Also (moved from NE8, since every allocation is atomic): the memory-layout and struct/array programs (`tests/test_memory_layout.rs`, `p8_structs_and_arrays` in `test_differential.rs`) and the NE7 control-flow functions that need memory (`first_non_digit`, `uses_void_return`, `side_effects`, `jump_in_operand`; remove them from the `drop` lists in `tests/test_native.rs`).
**Done (2026-10-04).** Load is a plain aligned load (atomic on x86); store and exchange use `xchg` (a full barrier, as sequentially consistent atomics need); add is `lock xadd`; compare-exchange `lock cmpxchg`. Alignment is checked before bounds, matching wasmtime's report for an access that is both. Native memory is never shared until NE15, so, exactly as wasm specifies for unshared memory, `memory.atomic.notify` wakes nobody and returns 0, and `memory.atomic.wait32` traps (`atomic wait on non-shared memory`, which locking a held lock in one thread reaches). Running: the atomics program, the struct/array cases (minus `test_mixed` for i64 arithmetic, NE12, and `test_floats`, NE13), `quicksort`, `accounts`, the four NE7 functions, and traps for misaligned and out-of-bounds atomics, bad unlocks, a lock on a non-lock word, a double lock, and a store into the reserved block. The harness now computes the expected results in one instance, in call order, as the generated `main` runs them (results like heap addresses depend on earlier calls). Mutation-checked: 8 deliberate bugs, all caught after adding a program for a 4-byte atomic store's neighbours.

### Group D: the runtime surface

**NE10: WASI part 1.** Depends on: NE9.
Start-up captures argc, argv, and envp from the initial stack. Implement `fd_write`, `fd_read`, `fd_close`, `proc_exit`, `args_sizes_get`, `args_get`, `environ_sizes_get`, `environ_get`, `clock_time_get`, `random_get` as routines in `runtime.aipl`, in the WASI layout and errno convention.
Done when: the args/env, stdin, clock, and random programs from `tests/test_wasi.rs` and `tests/test_runner.rs` match natively (clock and random checked by property, as those tests do).
**Done (2026-10-04), together with NE11:** `std/io` imports `path_open` too, so nearly every printing program needs both. `aipl_src/native/wasi.aipl` implements the functions as native routines over system calls, following wasmtime (what `aipl-run` uses) closely enough to compare output byte for byte: a pointer the program passes is checked before use and a bad one traps with wasmtime's words and numbers (`Pointer out of bounds: Region { start: S, len: L }`, then `Pointer not aligned to N: ...`; bounds before alignment; `*_sizes_get` writes the size before the count), which needed a decimal printer in the trap path (`runtime.emit_region_trap_routine`); `fd_read` into a buffer outside memory reads first, as wasmtime does, so empty input is no error and otherwise the region's length is what was read; Linux errors map to WASI errnos through a table (AIPL programs only see -1). The harness passes arguments, an environment, and stdin, and resolves imports, so test programs use the standard library and print: printing, numbers, raw writes to fds 1/2, bad and closed descriptors, stdin (empty, small, 120 KB), the command line and environment (including empty values and none at all), clocks and randomness by property (clock values compared through their 32-bit halves until NE12), and the pointer traps.

**NE11: WASI part 2, files and the access rules.** Depends on: NE10.
`path_open` and `path_unlink_file` through `openat`/`unlinkat`, with fd 3 = working directory and fd 4 = `/`, and the decided file-access rules: absolute paths allowed by default and refused with `--sandbox`; no path escapes its directory with `..`. The sandbox flag reaches the executable from the compiler (a byte in the data segment).
Done when: `word_count`, `word_freq`, the file programs in `test_wasi.rs`, and the `--sandbox` cases in `test_runner.rs` match natively.
**Done (2026-10-04), except `word_freq`,** whose hash map needs i64 comparisons and so moves to NE12. WASI descriptors index a table of Linux descriptors: 0-2 stdio, 3 the working directory, 4 `/` (omitted with sandbox; `native.compile_with` takes the flag, which NE18 wires to `--sandbox`); new files take the lowest free number, as wasmtime does; closing 0-2 frees the number but keeps the process's stdio, so trap messages still reach stderr. Paths resolve with `openat2(RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)`, so the kernel itself refuses `..` escapes, absolute paths under fd 3, and symlinks leading out; `path_unlink_file` opens the parent that way and unlinks the last component inside it (unlinkat has no such flag). The harness creates files and symlinks per run in a directory one level down and compares everything left under its parent after the `aipl-run` and native runs, so a write outside would show; a separate check requires every escape refused, with and without the sandbox. Programs: `word_count` (default, named, missing, and a 200 KB input; sandboxed), write-then-read, missing files, subdirectories and `./`, escapes (`..`, `sub/../..`, a symlink to `..`, absolute), symlinks inside the directory, deletes (including escapes, a directory, and a symlink itself), a read-only descriptor, descriptor numbering, absolute paths with and without the sandbox, and `aipl_src/file_io.aipl`'s self-test. Found on the way: `openat2` refuses a mode unless it creates a file; and the harness's expected-value instance lacked aipl-run's preopens. Mutation-checked: 14 deliberate bugs (bounds, alignment, size order, NULs, RESOLVE_BENEATH, O_NOFOLLOW, descriptor numbering, the sandbox, path termination, reads outside memory, clock units, unlink's parent check, the environment count), all caught after adding two programs and fixing a clock threshold that was off by a factor of 1000 (it passed any clock value).

### Group E: the rest of the instruction set

**NE12: i64.** Depends on: NE9.
All `i64` arithmetic, comparisons, `i64.extend_i32_s/u`, i64 loads and stores, with the same trap rules as NE6.
Done when: `tests/test_i64.rs` programs match natively. Also: the NE10 clock program can compare whole i64 times. (`word_freq`, moved here from NE11, also needs `call_indirect` for its sort comparator, so it is NE14's.)
**Done (2026-10-04).** The i32 instructions' 64-bit forms: `div_s` traps on zero and `INT64_MIN / -1`, `rem_s` by -1 gives 0 without reaching `idiv`, shifts use x86's 6-bit count masking (as wasm), comparisons push an i32; `extend_i32_s` sign-extends (`movsxd`) and `extend_i32_u` clears the high half, which matters because an i32 made by `i32.wrap_i64` keeps the i64's high half in its slot. The function checker compares i64 results (and, ready for NE13, floats by their bits). Running: the `test_i64.rs` cases, the struct case with an i64 field (`test_mixed`), the i64 traps, the clock program comparing whole i64 times, every i64 operator on all pairs of 16 edge values in hand-built wasm, and extending after wrapping. Mutation-checked: 9 deliberate bugs, all caught.

**NE13: floats.** Depends on: NE9.
SSE2 encoder forms in `x64.aipl`, then `f32`/`f64` constants, arithmetic, comparisons (NaN compares false, as wasm), loads/stores, `f64.convert_i64_s`, `i64.trunc_f64_s` (traps on NaN and out of range, exactly at wasm's bounds), and the reinterprets.
Done when: the float programs in `test_differential.rs`, `examples/matrix_mult.aipl`, and the float-literal cases match natively.
**Done (2026-10-04).** x64.aipl gained the SSE2 forms (`movq`/`movd` between general and xmm registers, `addsd`..`divss`, `ucomisd`/`ucomiss`, `cvtsi2sd`, `cvttsd2si`), checked against GNU as over all 16 xmm registers and added below the existing tests (line 404 unmoved). A slot holds a float's bits; arithmetic moves them through xmm0/xmm1. Comparisons follow wasm's NaN rule (false, except `ne`): `ucomi` sets ZF, PF, and CF when unordered, so `eq` needs ZF and not PF, `ne` not-ZF or PF, and the orderings use above / above-or-equal with the operands swapped for `lt`/`le`. `i64.trunc_f64_s` traps on NaN (`invalid conversion to integer`) and outside [-2^63, 2^63) (`integer overflow`), checked before `cvttsd2si`; the reinterprets emit nothing. f64 results are compared by their bits, so NaN payloads and -0.0 count. Running: the `test_differential.rs` float cases plus NaN, infinity, and -0.0 arithmetic and comparisons, conversions at both ends of the range, a struct with f64/f32 fields, `matrix_mult`, the 200 float literals of the self-host test, the conversion traps, and every f32 and f64 operator over all pairs of 14 edge values (subnormals, infinities, NaN) in hand-built wasm. Mutation-checked: 10 deliberate bugs (parity flags, operand order, strictness, single vs double, the conversion bounds and NaN check), all caught; and 3 for the SSE2 encodings (prefix before REX, the rounding vs truncating conversion, the signaling compare).

**NE14: function references.** Depends on: NE9.
The function table, element section, and `call_indirect` with its type check (trap on mismatch, as wasm).
Done when: `tests/test_refs.rs` programs and `vec.sort_by` users (`word_freq`, `std/vec` tests) match natively.
**Done (2026-10-04).** The table is a data array of 16-byte entries (code address, canonical type id, padding), type -1 for an empty slot, with addresses written in once the code layout is known. A function type's canonical id is the first structurally equal type's index, since wasm compares function types by structure. `call_indirect` zero-extends the index, then traps on an index past the table (`undefined element: out of bounds table access`), an empty slot (`uninitialized element`), and a type mismatch (`indirect call type mismatch`), in that order, before calling through the address. Running: `REFS_PROGRAM` (references through parameters, arrays, and struct fields; reference equality), the `std/vec` self-tests (`sort_by` with a comparator), `word_freq`, references to functions with i64 and f64 parameters, and hand-built tables: a call, a call through a structurally equal type, and the three traps (one with index -1). The function checker now parses through the resolver, so generic and importing programs are checked function by function too. Mutation-checked: 9 deliberate bugs, all caught after adding an index equal to the table size and an element segment starting past slot 0.

### Group F: threads

**NE15: thread creation.** Depends on: NE11, NE14.
Threaded modules: shared memory, passive data plus the start function's `memory.init`, the per-thread global, and `thread-spawn` as `clone` with an `mmap`ed stack that runs `wasi_thread_start(tid, record)`. A thread that traps or exits ends the process, as under `aipl-run`.
Done when: `join_returns_the_worker_result` and `concurrent_allocations_never_overlap` from `test_threads.rs` match natively.

**NE16: waiting and locks.** Depends on: NE15.
`memory.atomic.wait32` and `notify` as `futex` wait/wake; the lock and join paths built on them.
Done when: every `tests/test_threads.rs` program matches natively, repeatedly (run each 20 times to shake out races).
**NE15 and NE16 done together (2026-10-04)**, since even NE15's `join` waits. Threads are `clone(VM|FS|FILES|SIGHAND|THREAD|SYSVSEM|SETTLS)` on an 8 MiB `mmap`ed stack whose top holds `wasi_thread_start`'s arguments; the new thread runs the start function and `wasi_thread_start(tid, arg)` (tids from 1, as aipl-run's), then `exit`s alone; a trap or `proc_exit` anywhere is `exit_group`, ending the program as under aipl-run. Wasm globals are per instance and aipl-run gives each thread an instance, so each thread's FS base points at its own globals block (main: in the data; spawned: the low end of its stack, initialised before the clone), and `global.get/set` are FS-relative (x64.aipl gained the FS prefix, checked against GNU as). The imported shared memory is set up like an own one; passive segments are placed in the data before translation so `memory.init` (both ranges checked) can embed their addresses. On shared memory `wait32` is `futex` wait (0 woken, 1 the word differed, 2 timed out; an interrupted wait counts as woken, callers loop) and `notify` is `futex` wake; unshared memory keeps NE9's rules. Made thread-safe: `memory.grow` is a compare-and-swap loop on the size cell (threads allocate, and so grow, at once), and `path_open` claims its descriptor slot with `cmpxchg`. Running, 20 times each: the `test_threads.rs` programs (counter, join, mutex, concurrent allocation, concurrent printing with whole texts intact); plus a handshake that finishes only if two threads truly run at once, a trap and `sys.exit` inside a thread, forty threads, and on shared memory directly: waits that time out and that find a different value, notify with no waiters, an unaligned wait, and `memory.init` (a copy, both bounds, an empty copy at the end). Mutation-checked: 11 deliberate bugs, all caught after adding three programs the first round showed were missing: the main thread of a threaded module printing (its own globals), a spawned thread reading a global's initial value (hand-built), and eight threads growing memory 400 times at once (the sum of the old sizes catches a lost update). Two changes are not observable and were not counted: a thread skipping the start function (it only re-checks the init flag) and a futex wait that returns at once (callers loop on their condition, so only CPU time differs).

### Group G: finishing

**NE17: the compiler, natively.** Depends on: NE16 and NE12-NE13.
Build `aipl_src/driver.aipl` natively.
Done when: the native `aiplc` compiles every repository program to the same wasm as the Rust toolchain, compiles itself to identical wasm, and building it natively twice gives identical executables (reproducible). Record its size and speed against the launcher build in PROGRESS.md.
**Done (2026-10-04).** `tests/test_native.rs` (`the_compiler_runs_natively`) builds `aipl_src/driver.aipl` natively, twice (byte-identical: reproducible), runs it from the repository root on every program the Rust toolchain compiles (29, including the native backend's own modules), requiring the Rust toolchain's bytes for each, then has it compile itself: the output is exactly the wasm it was built from, and translating that wasm natively again gives the same executable, so the whole AIPL compiler reaches a fixpoint with no Rust and no wasmtime. Usage and resolve errors behave as the wasm command's. Measured (release builds, this machine): the native `aiplc` is 347,916 bytes against 18,228,395 for the launcher build (52 times smaller); compiling `word_count` takes 8 ms against 37 ms (no engine to load or compile at start-up); compiling the compiler itself takes 116 ms against 106 ms (the baseline translator's code is about 10% slower than Cranelift's on long work, well inside the 2-5 times the plan expected).

**NE18: make it the default.** Depends on: NE17.
`aipl compile --exe` produces a native executable on Linux x86-64 (and the launcher bundle elsewhere, or with `--target wasm`); `--sandbox` works for both. Update AIPL_SPEC.md (a native backend section beside 6.5), README, PROGRESS, LANGUAGE_GAPS, and the audit.
Done when: the full test suite passes, `test_runner.rs` covers both kinds of executable, and the docs describe the native path.
**Done (2026-10-04).** `aipl compile --exe` writes native code on Linux x86-64 and the launcher bundle elsewhere; `--target native|wasm` chooses explicitly (`--target native` elsewhere is an error); `--sandbox` works for both. `src/native.rs` runs `native.aipl` in the CLI's embedded wasmtime and the CLI sets the execute bit (WASI cannot). `test_runner.rs` runs, for each kind: `word_count` (from another directory, and sandboxed), the AIPL compiler as `aiplc` compiling a program to the Rust toolchain's bytes, a trap's exact message, and a threaded program; it checks each file is the kind asked for (native: an ELF with no launcher inside; wasm: the launcher bundle) and that the default is native here. AIPL_SPEC.md 6.6 describes the native path.

## Testing

- A Rust test helper (NE5) compiles a program as wasm under `aipl-run` and natively, and requires identical stdout, stderr, and exit status. Every task adds its programs to one list, so earlier tasks stay covered.
- During development, `objdump -d` (if installed) is a useful aid for checking encodings, but tests must not depend on it.
- Instruction encodings get small unit tests of their own (known instruction → known bytes), since one wrong byte in an encoder is otherwise hard to find.

## Risks and how they are handled

- **Encoding mistakes** are the most likely bug class: covered by the per-instruction byte tests and by whole-program comparison.
- **Floating-point corner cases** (NaN, the trapping `i64.trunc_f64_s` range): follow the wasm spec exactly; the existing float differential tests cover them.
- **Threads** are the hardest part; they come last (NE15-NE16), when everything else is solid.
- **Scope:** this is several thousand lines of AIPL. The tasks are ordered so each is useful and testable on its own; stopping after any of them leaves a working toolchain.
