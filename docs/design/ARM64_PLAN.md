# Native ARM64 backend: implementation plan

**Status: approved 2026-10-29, under way (NA1-NA3 done; NA4-NA8 begun: programs that print run on AArch64 Linux).** A design for adding native ARM64 (AArch64) execution to AIPL, beginning with Linux AArch64 ELF, while retaining the launcher bundle on macOS Darwin. The owner approved the plan with the recommended answers to section 6: Linux AArch64 ELF first with the launcher bundle staying the macOS answer; 64 KiB segment alignment for ARM64 only (x86-64's bytes do not change); a sibling `lower_arm64.aipl`; inline division checks; and GitHub's `ubuntu-24.04-arm` runner in the workflow.

Written following the Apple Silicon port and investigation documented in `docs/APPLE_SILICON_REPORT.md` and following the structure of `docs/design/NATIVE_BACKEND_PLAN.md`.

## At a glance

| Step | What | Status |
|---|---|---|
| NA1 | Plan approval & design freeze | **Done** (2026-10-29) |
| NA2 | `aipl_src/native/arm64.aipl`: AArch64 32-bit instruction encoder | **Done** (integer, memory, branch, and system forms; floating point waits for NA8) |
| NA3 | ELF writer updates in `aipl_src/native/elf.aipl`: 64 KiB page alignment & segment 0 at file offset 0 | **Done** (2026-10-29) |
| NA4 | `runtime_arm64.aipl`: AArch64 Linux syscalls, start-up stub, trap routine, fault handler | **Begun**: syscalls and the first executable (prints a line, exits 7) run on AArch64 Linux; start-up, traps, and the fault handler to come |
| NA5 | `lower_arm64.aipl`: Baseline instruction lowerer for integer arithmetic, locals, constants, memory | **Begun** (2026-10-30): a baseline lowerer, every value in a 16-byte slot on the machine stack; integers, locals, globals, memory, control flow, calls, `call_indirect`, memory.size and grow, and the i32 atomics; the driver is `native_arm64.aipl`; its optimizer: ARM64_OPT_PLAN.md |
| NA6 | Trap preservation: explicit checks for division-by-zero & signed overflow (`INT_MIN / -1`) | **Mostly done** (2026-10-30): the checks end in a trap routine (`backtrace_arm64.aipl`) that prints `<argv[0]>: <reason>`, the program's own failure message when it left one, the call chain with `file:line:col` positions, and exits 134; the same text as the x86-64 build and aipl-run, and an invalid exit status without a chain; a SIGSEGV handler makes an out-of-bounds access the same trap; running out of stack is the trap `call stack exhausted` (the main thread runs on a stack of its own, 8 MiB below a guard region like a spawned thread's, and each function's prologue compares `sp` with the thread's limit, kept in the thread pointer's block), and a bad pointer to a WASI function traps with wasmtime's `Pointer out of bounds: Region { start: S, len: L }` (or `Pointer not aligned to N`), all as the x86-64 build does; the process, socket, and thread operations are done (below) |
| NA7 | Control flow: blocks, loops, branches, calls, function references | Not started |
| NA8 | Floating point: `v0..v31` (scalar `s0..s31`, `d0..d31`) instructions | **Done** for the instructions wasm has (arithmetic, rounding, comparisons, conversions, truncations that trap, copysign); the encoder checks 11,772 words against Apple's assembler |
| NA9 | WASI routines (`wasi_arm64.aipl`): stdio, path_open, args, environ, clock, random | **Done for files** (2026-10-30): proc_exit, args, environ, clock, random, fd_write/fd_read through a descriptor table, fd_close, path_open (openat2 with the openat fallback), path_unlink_file, path_remove_directory, path_create_directory, path_rename, fd_readdir, path_filestat_get, poll_oneoff, aipl_sys_v1 chmod, and the process operations (`procs_arm64.aipl`: spawn by `clone`, stream, read, write, close, wait, exec; `aipl run`, `test`, `watch`, and `eval` work on AArch64 Linux), all as the x86-64 build does, a bad pointer included (NA6); `--sandbox` done (no `/` preopen, `openat2` required, a start-up probe) |
| NA10 | Sockets & fresh serving (`sockets_arm64.aipl`, `fresh_arm64.aipl`): `aipl_net_v1` over AArch64 Linux | **Done** (2026-10-30): listen, accept, recv, send, close with the `--listen` grants, and `--serve` (a fresh copy per connection, `requests=N`, `temporary`), as the x86-64 build does; run on AArch64 Linux with nc clients, and in tests from this machine's network (a host, or qemu-user) |
| NA11 | Threads & atomics: `clone`, `futex`, load/store acquire/release barriers | **Done** (2026-10-30): `thread-spawn` (a stack with a guard region, a per-thread block of globals and results behind `TPIDR_EL0` by clone's SETTLS), futex wait and wake on shared memory, `memory.init`, the atomics; `examples/spectralnorm_par.aipl` gives the sequential result on 1, 3, and 8 threads, and the workers of `tests/test_threads.rs` pass 30 runs in 30 on AArch64 Linux |
| NA12 | End-to-end integration: `aipl compile --target native-linux-arm64`, test suite un-gating, CI runner configuration | **Begun** (2026-10-30): `--target arm64` in both toolchains (the self-hosted one defaults to it on an AArch64 machine); the Rust suite passes on AArch64 Linux (475 tests, the others being x86-64's); the compiler compiles itself on AArch64 Linux to the same bytes (tools/arm64_bootstrap.sh); a GitHub `ubuntu-24.04-arm` job and a checked-in AArch64 seed to come |

---

## 1. Goal & Context

The existing native backend (`aipl_src/native/`, 6,666 lines across 9 files) emits static x86-64 Linux ELF binaries directly from WebAssembly bytecode without external dependencies, libc, or runtimes. On Linux x86-64, this produces compact (few KB to ~3.7 MB), standalone executables.

The goal of this plan is to bring the identical capability to **Linux AArch64 (ARM64)**:
```
prog.aipl --(resolver, generics, codegen: existing AIPL)--> prog.wasm --(new ARM64 backend)--> prog (AArch64 ELF)
```

### Key Findings from Apple Silicon Porting (Tasks 1–4)
1. **Linux arm64 in Docker Desktop** runs natively on Apple Silicon at full hardware speed. All non-native tests (61 of 62 test targets) pass cleanly.
2. **Rosetta 2 Linux Limitation**: Apple's Rosetta 2 translation environment fails on AIPL's x86-64 ELF binaries because of an internal check (`assertion failed [segment_file_offset == 0]: first load segment not at file offset 0 (ElfMapper.cpp:399)`). The ARM64 ELF writer must follow standard segment conventions starting the first load segment at file offset 0.
3. **macOS Executables via Launcher Bundle**: On macOS Darwin (Apple Silicon), the launcher bundle (`aipl-run` + appended wasm payload) already runs cleanly, preserves its ad-hoc code signature, and executes at near-native Cranelift JIT speed.

Therefore, the highest ROI is to build **Linux AArch64 ELF first**. macOS Mach-O native emission is deferred to a distinct, later phase.

---

## 2. Linux AArch64 ELF: Architecture & Components

### 2.1 What Stays (Reused 100%)
- **`wasm_reader.aipl` (916 lines)**: Decodes wasm bytecode into typed records (`Body`, `Instr`, types, imports, exports, memory, data segments). It is completely architecture-independent and requires zero changes.
- **`native.aipl` driving pipeline (686 lines)**: Orchestrates section parsing, data segment layout, position tables, and memory reservations. It will be parameterized by target architecture.

### 2.2 What is Updated
- **`elf.aipl` (144 lines)**:
  - **Segment Alignment**: x86-64 Linux assumes 4 KiB pages (`0x1000`). AArch64 Linux kernels support 4 KiB, 16 KiB (standard on Apple Silicon Linux / Asahi / Linuxkit), and 64 KiB pages (standard in enterprise distributions like RHEL AArch64). The ELF writer must align segments to **64 KiB** so binaries run portably across all AArch64 page sizes.
  - **Offset 0 Mapping**: The first `PT_LOAD` segment (containing the ELF header, program headers, and read-only code) must start at file offset `0x0000` (mapped `RX` or `R`). The data segment follows on the next page-aligned boundary (`RW`). This complies with Rosetta 2's `ElfMapper` requirement and standard POSIX conventions.

### 2.3 What is New

#### A. AArch64 Instruction Encoder (`aipl_src/native/arm64.aipl`)
Replaces `x64.aipl` (752 lines). Unlike x86-64's variable-length (1–15 byte) instructions, AArch64 instructions are uniformly **32-bit (4-byte) little-endian words**.
The encoder emits byte-exact encodings for:
- **Data processing (register & immediate)**: `add`, `sub`, `mul`, `sdiv`, `udiv`, `and`, `orr`, `eor`, `lsl`, `lsr`, `asr`, `mov`, `movz`, `movk`, `movn`.
- **Memory load/store**: `ldr`, `str`, `ldrb`, `strb`, `ldrh`, `strh`, `ldp`, `stp`.
- **Branching & control**: `b`, `b.cond` (`b.eq`, `b.ne`, `b.lt`, `b.ge`, etc.), `cbz`, `cbnz`, `bl`, `blr`, `ret`.
- **Comparisons & condition setting**: `cmp`, `cmn`, `cset`, `csel`.
- **System & barriers**: `svc #0`, `dmb`, `isb`, `brk #0`.
- **Floating point (FP/SIMD)**: `fadd`, `fsub`, `fmul`, `fdiv`, `fcmp`, `fmov`, `fcvt*`.

#### B. Register Model & Calling Convention (`lower_arm64.aipl`)
AArch64 provides 31 general-purpose 64-bit registers (`x0`–`x30`), the zero register (`xzr`), stack pointer (`sp`), and 32 FP/SIMD 128-bit registers (`v0`–`v31`).

Allocation mapping for AIPL's baseline compiler:
| Register(s) | Role | Notes |
|---|---|---|
| `x0`–`x7` | Argument passing & returns / Scratch | Caller-saved. Arguments 0–7; results returned in `x0`. |
| `x8` | Linux System Call Number / Indirect Result | Holds syscall number during `svc #0`. |
| `x9`–`x15` | Scratch / Temporary pool | Caller-saved. Used for value-stack scratch registers. |
| `x16`–`x17` | Intra-procedure call scratch (`IP0`, `IP1`) | Available for jump trampolines or code veneer. |
| `x18` | Reserved (Platform register) | Unused / reserved by Linux ABI. |
| `x19`–`x27` | Callee-saved locals | Preserved across calls; hold hot locals and module globals. |
| `x28` | Linear Memory Base (`r15` equivalent) | Dedicated base pointer pointing to start of Wasm linear memory. |
| `x29` | Frame Pointer (`FP`) | Callee-saved. Points to caller's frame record. |
| `x30` | Link Register (`LR`) | Holds return address for `ret`. |
| `sp` | Machine Stack Pointer | **Must be 16-byte aligned** on every load/store through `sp`. |
| `v0`–`v7` | FP arguments & return values | 32-bit floats (`s0..s7`), 64-bit doubles (`d0..d7`). |
| `v8`–`v15` | Callee-saved FP registers | Lower 64 bits (`d8..d15`) preserved across calls. |
| `v16`–`v31` | Caller-saved FP scratch | Available for float value-stack operations. |

#### C. SP Alignment Discipline
ARM64 hardware enforces that whenever `sp` is used as a base register for memory access (`ldr`/`str`), `sp` must be 16-byte aligned, or the processor raises an `Alignment Fault`.
In AIPL's single-pass baseline compiler:
- All frame adjustments (`sub sp, sp, #N`) must allocate multiples of 16 bytes.
- Pushing/popping 8-byte value stack slots will either:
  1. Pair pushes using `stp xA, xB, [sp, -16]!` and `ldp`, or
  2. Use a dedicated register (e.g., `x27`) as the Wasm operand stack pointer, while keeping machine `sp` strictly 16-byte aligned for calls and frame records. (Option 1 is recommended to avoid dedicating an extra register).

#### D. Traps Preservation
WebAssembly mandates specific trap behavior that differs from ARM64 hardware defaults:
1. **Integer Division by Zero**:
   - On x86-64: `idiv`/`div` automatically raises `#DE` (SIGFPE).
   - On ARM64: `sdiv` and `udiv` **do not trap**; dividing by zero silently returns `0`.
   - *Design*: The compiler must emit an explicit check before every `div_u`, `div_s`, `rem_u`, `rem_s`:
     ```asm
     cbz  w_divisor, .Ltrap_div_by_zero
     ```
2. **Signed Division Overflow (`INT_MIN / -1`)**:
   - On x86-64: `idiv` on `0x80000000 / -1` raises `#DE` (SIGFPE).
   - On ARM64: `sdiv` on `INT_MIN / -1` silently returns `INT_MIN`.
   - *Design*: For `i32.div_s` and `i64.div_s`, test dividend and divisor:
     ```asm
     cmp  w_dividend, #0x80000000
     ccmp w_divisor, #-1, #0, eq
     b.eq .Ltrap_integer_overflow
     ```
3. **Out-of-Bounds Linear Memory**:
   - Exactly as in `runtime.aipl` on x86-64: reserve 8 GiB virtual address space via `mmap(PROT_NONE)`. `mprotect` the committed portion up to the current linear memory size. Accesses beyond the committed limit fault.
   - An `rt_sigaction` handler catches `SIGSEGV` and jumps to the trap routine (`Pointer out of bounds`).

#### E. Linux AArch64 System Calls (`runtime_arm64.aipl`, `wasi_arm64.aipl`)
Linux AArch64 uses the standard asm-generic syscall numbering (different from x86-64):
| Operation | AArch64 Syscall # | x86-64 Syscall # | Arguments |
|---|---|---|---|
| `write` | 64 | 1 | `x0`=fd, `x1`=buf, `x2`=count |
| `read` | 63 | 0 | `x0`=fd, `x1`=buf, `x2`=count |
| `close` | 57 | 3 | `x0`=fd |
| `openat` | 56 | 257 | `x0`=dfd, `x1`=filename, `x2`=flags, `x3`=mode |
| `openat2` | 437 | 437 | `x0`=dfd, `x1`=filename, `x2`=how, `x3`=size |
| `unlinkat` | 35 | 263 | `x0`=dfd, `x1`=pathname, `x2`=flags |
| `exit_group` | 94 | 231 | `x0`=status |
| `mmap` | 222 | 9 | `x0`=addr, `x1`=len, `x2`=prot, `x3`=flags, `x4`=fd, `x5`=off |
| `mprotect` | 226 | 10 | `x0`=addr, `x1`=len, `x2`=prot |
| `munmap` | 215 | 11 | `x0`=addr, `x1`=len |
| `clock_gettime`| 113 | 228 | `x0`=clk_id, `x1`=tp |
| `getrandom` | 278 | 318 | `x0`=buf, `x1`=buflen, `x2`=flags |
| `rt_sigaction` | 134 | 13 | `x0`=signum, `x1`=act, `x2`=oldact, `x3`=sigsetsize (8) |
| `clone` | 220 | 56 | `x0`=flags, `x1`=stack, `x2`=ptid, `x3`=tls, `x4`=ctid |
| `futex` | 98 | 202 | `x0`=uaddr, `x1`=op, `x2`=val, `x3`=utime, `x4`=uaddr2, `x5`=val3 |

Invocation on ARM64:
```asm
mov  x8, #SYS_NUM
svc  #0
```

---

## 3. macOS Mach-O: Why Separate and Deferred

Unlike Linux, macOS Darwin does not provide a stable, supported direct system call ABI:
1. **No Stable Syscall Interface**: Apple frequently changes kernel syscall numbers and ABI calling conventions between macOS releases. Invoking raw `svc #0x80` traps can be blocked or cause undefined behavior.
2. **Mandatory Dynamic Linking**: All macOS executables are required to link dynamically against `/usr/lib/libSystem.B.dylib` (which provides POSIX wrappers, pthreads, and malloc). A pure standalone static binary without dynamic loading is not supported on modern macOS.
3. **Complex Mach-O Object Format**: Generating a native Darwin executable requires implementing a Mach-O 64-bit generator:
   - Mach-O header (`mach_header_64`) with `CPU_TYPE_ARM64`.
   - Segment load commands: `LC_SEGMENT_64` for `__PAGEZERO`, `__TEXT` (`__text`, `__cstring`), `__DATA` (`__data`, `__bss`), `__LINKEDIT`.
   - Dynamic loader commands: `LC_LOAD_DYLINKER` (`/usr/lib/dyld`), `LC_LOAD_DYLIB` (`/usr/lib/libSystem.B.dylib`), `LC_MAIN` (entry point).
   - Dynamic symbols and chained fixups (`LC_DYLD_CHAINED_FIXUPS` / `LC_DYLD_EXPORTS_TRIE`) to bind calls to `_write`, `_read`, `_mmap`, `_exit`, etc.
4. **Mandatory Code Signing**: Every executable on Apple Silicon must carry an `LC_CODE_SIGNATURE` command containing valid SHA-256 code directory hashes (CDHashes), or the Darwin kernel terminates the process with `SIGKILL` ("Killed: 9").

### Is the Launcher Bundle Sufficient for macOS?
**Yes, entirely.** As verified in Task 3:
- `aipl compile --exe prog.aipl -o prog` on macOS produces a 14.2 MB Mach-O executable containing the compiled `aipl-run` engine and the wasm bytecode.
- It executes at full Cranelift JIT speed, preserves valid ad-hoc code signatures, runs CLI programs and long-running servers, and complies with all macOS Gatekeeper/AMFI security rules.
- Developing a custom Mach-O emitter represents substantial engineering complexity for very low marginal benefit compared to Linux AArch64 ELF.
- **Recommendation**: Retain the launcher bundle as the definitive macOS standalone solution, and concentrate native compiler work exclusively on Linux AArch64 ELF.

---

## 4. Testing & Validation Strategy

1. **Docker Desktop Native Verification**:
   - Runs `linux/arm64` containers locally on Apple Silicon at 100% native execution speed.
   - Verified via `tools/container_test.sh arm64`.
2. **GitHub Actions CI Runners**:
   - **`ubuntu-24.04-arm`**: GitHub provides native ARM64 Ubuntu runners for public repositories. This gives native CI builds and tests with no emulation overhead.
   - **`qemu-aarch64-static` User-Mode Emulation**: For standard x86-64 runners (`ubuntu-latest`), install `qemu-user-static` and `binfmt-support`. This allows testing ARM64 binaries on standard GitHub Actions runners.
3. **Differential Fuzzing (`tools/run_fuzz.py`)**:
   - `run_fuzz.py` compiles randomized AIPL programs to both wasm (executed under `aipl-run` / VM) and native binaries, asserting bitwise-identical stdout and exit codes.
   - Running `run_fuzz.py` on `linux/arm64` provides automated validation against thousands of generated edge cases (including division by zero, floating point edge cases, and memory bounds).
4. **Un-gating Existing Native Tests**:
   - Tests in `tests/test_native.rs`, `tests/test_native_elf.rs`, `tests/test_benchmarks.rs`, and `tests/test_eval_expr.rs` currently gated with:
     ```rust
     if !cfg!(all(target_os = "linux", target_arch = "x86_64")) { return; }
     ```
     will be broadened to:
     ```rust
     if !cfg!(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))) { return; }
     ```

---

## 5. Size of the Work & Phased Tasks

### Size Comparison
The existing x86-64 native backend:
```
  144 aipl_src/native/elf.aipl
  290 aipl_src/native/fresh.aipl
 2328 aipl_src/native/lower.aipl
  686 aipl_src/native/native.aipl
  378 aipl_src/native/runtime.aipl
  323 aipl_src/native/sockets.aipl
  849 aipl_src/native/wasi.aipl
  916 aipl_src/native/wasm_reader.aipl
  752 aipl_src/native/x64.aipl
 6666 total lines
```

Of these:
- **1,602 lines** (`wasm_reader.aipl` + `native.aipl`) are shared or require minimal changes.
- **144 lines** (`elf.aipl`) need minor updates (page alignment & offset 0).
- **~4,900 lines** of target-specific code (`x64`, `lower`, `runtime`, `wasi`, `sockets`, `fresh`) will have corresponding AArch64 implementations.

### Proposed Step-by-Step Tasks (NA1–NA12)

Like the original NE1–NE18 tasks in `docs/design/NATIVE_BACKEND_PLAN.md`, each task is independently reviewable and testable:

1. **NA1: Plan Approval**: Owner reviews and approves this design.
2. **NA2: `arm64.aipl` Encoder**: Implement basic integer arithmetic, data transfer, and branch instructions.
   *Done when*: a test checks byte-exact instruction words against an assembler. **Done 2026-10-29**: against Apple's (`clang -arch arm64` on the Mac mini, read back with `otool`), 9,289 words over registers x0-x30 in nine groups (moves and `mov_imm`'s movz/movn/movk sequences, register ALU, multiply and divide, immediate ALU, shifts and extensions, conditional select and compare, loads and stores in every addressing form the lowering needs, branches with patching, system). `tools/arm64_vectors.py` writes the forms as encoder calls and as assembly in one place and regenerates the test region of arm64.aipl (`ssh pennylane` by default); tests/test_native_arm64.rs runs it in the VM and compiled to wasm, on any machine.
3. **NA3: `elf.aipl` 64 KiB Alignment**: Update ELF writer to support 64 KiB pages and segment 0 at file offset 0.
   *Done when*: Minimal static exit binary runs cleanly on Linux AArch64 and passes Rosetta 2 `ElfMapper` check. **Done 2026-10-29**: `elf.image_arm64` (machine 183, segments on 64 KiB boundaries, the headers read-only from file offset 0, `elf.image`'s x86-64 bytes unchanged) builds the executable that `runtime_arm64.hello_exe` makes, and it ran on the Mac mini in a `linux/arm64` container (tests/arm64/mod.rs runs AArch64 code on an AArch64 Linux host, under qemu-user, or over ssh with `AIPL_ARM64_SSH=pennylane`; without one the test says it did not run). The Rosetta check concerns x86-64 binaries and is the earlier work.
4. **NA4: `runtime_arm64.aipl`**: Implement AArch64 syscall wrappers, start-up code, `argv[0]` handling, and trap routine.
   *Done when*: Standalone ELF binary prints a test string and exits cleanly with requested status. **The first half done 2026-10-29** (the syscall wrappers and the executable that prints and exits 7, run on AArch64 Linux); start-up, `argv[0]`, and the trap routine remain.
5. **NA5: `lower_arm64.aipl` Core**: Translate basic blocks with integer arithmetic (`add`, `sub`, `mul`, bitwise), constants, and locals.
   *Done when*: Basic expressions (`(+ 1 2)`) evaluate and produce identical output to wasm.
6. **NA6: Trap Semantics**: Implement explicit division-by-zero checks, signed overflow traps, and `SIGSEGV` guard-page memory traps.
   *Done when*: Division by zero and out-of-bounds array access produce exit status 134 and exact error strings.
7. **NA7: Control Flow**: Blocks, loops, `br`, `br_if`, function calls, and indirect table calls.
   *Done when*: Loop benchmarks (e.g., `spigot`, `quicksort`) compile and pass.
8. **NA8: Floating Point**: Scalar FP operations via `v0..v31` (`s0..s31`, `d0..d31`).
   *Done when*: Float math tests (`math_core.aipl`, `nbody`) match wasm output bit-for-bit.
9. **NA9: `wasi_arm64.aipl`**: WASI file I/O, directory sandboxing (`openat2`), clock, and arguments.
   *Done when*: `word_count.aipl` compiles to native ARM64 ELF and counts words correctly.
10. **NA10: Networking & Sockets**: AArch64 implementation of `aipl_net_v1` (`listen`, `accept`, `recv`, `send`, `close`) and `--serve` fresh worker.
    *Done when*: `serve_demo.aipl` answers HTTP requests natively on Linux AArch64.
11. **NA11: Threads & Atomics**: Thread spawning (`clone`), futex wait/wake, and memory barriers.
    *Done when*: `tests/test_threads.rs` passes natively on Linux AArch64.
12. **NA12: CI & Fuzzer Integration**: Enable `ubuntu-24.04-arm` runner and run 3,000 cases of `run_fuzz.py`.
    *Done when*: Full native test suite passes with 0 failures on ARM64 Linux.

---

## 6. Open Questions for the Owner

1. **Target Architecture Focus**: Do you approve focusing purely on **Linux AArch64 ELF** for the native compiler while maintaining the **launcher bundle** for macOS?
2. **Page Size Standard**: Should `elf.aipl` adopt 64 KiB page alignment across all architectures, or retain 4 KiB for x86-64 and use 64 KiB for AArch64? (64 KiB is standard for portable Linux AArch64 and works on 4K/16K/64K kernels).
3. **Compiler Architecture**: Should `lower_arm64.aipl` be a direct sibling of `lower.aipl` (duplicating the wasm walking structure for clarity and independence, as done in Liftoff/Winch), or would you prefer extracting a shared abstract lowerer?
4. **Division Trap Implementation**: Do you prefer explicit inline conditional branches (`cbz` + comparison) for integer division, or an alternative signal-based approach? (Inline checks are recommended for predictability and performance).
5. **CI Hardware Preference**: Would you prefer adding GitHub's hosted `ubuntu-24.04-arm` runner directly to `.github/workflows/ci.yml`, or using `qemu-user-static` under x86-64 runners, or both?
