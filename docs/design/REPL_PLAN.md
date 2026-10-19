# Self-Hosted REPL and WebAssembly Interpreter

**Status (2026-10-19): planned, not started.** Order agreed with the
owner: after the standard library (STDLIB_PLAN.md L7) and the stage-0
seed refresh (docs/LAUNCH_CHECKLIST.md section 6). The **interpreter**
comes first and matters most: it gives the self-hosted `aipl` `eval`,
`run`, and `test` with no Rust and no wasmtime (docs/LAUNCH_CHECKLIST.md
section 4, "A binary that works wherever it is"). The **REPL** on top is
low priority and ships marked **experimental**, in its help text and its
banner: AIPL is written by agents through files, the compiler, and its
errors, not typed at a prompt by people; the REPL is for people learning
the language.

Corrections to the plan below, from checking it against the code
(2026-10-19):

1. **The instruction set (3.1) is not complete.** Every allocation
   compiles to `i32.atomic.rmw.add` (AIPL_SPEC.md 6.2), and the native
   backend implements the atomics (aipl_src/native/lower.aipl, NE9). Take
   the set from what lower.aipl accepts, which is what codegen.aipl
   emits, not from a list. `thread.spawn` is refused with a clear error.
2. **Value printing (4.2, 4.3) is already solved.** Every value prints
   with `{x}` (AIPL_SPEC.md 4.K), and `aipl eval -e` gets an expression's
   type from the checker (src/evalexpr.rs); the REPL uses the same, not
   a probe over scalar types.
3. **Session state, stated plainly.** Each input runs the session again
   from the top, hiding the output earlier inputs already gave; file
   writes, the time, and random numbers happen again each time. The
   banner says so.
4. **No line editing.** The native runtime has no terminal raw mode, so
   there are no arrow keys or history; `rlwrap aipl repl` gives both.
5. **Seed-compatible source.** The interpreter is compiled into the
   self-hosted `aipl`, so it is compiler source and may use only what
   the seed knows: hence the seed refresh first.
6. **Its oracle** is the same output as wasmtime and native on every test
   program (no Rust twin, no byte parity: it executes, it does not
   compile).

This plan defines the design for an interactive REPL and execution engine written entirely in AIPL, shipping as part of the self-hosted native binary (`aipl_src/aipl.aipl` / `bootstrap/aipl-linux-x86_64`), with zero dependency on Rust, Cargo, or external runtimes like Wasmtime.

---

## 1. Motivation and Context

Currently, the AIPL toolchain has two halves:
1. **The Rust toolchain (`src/`):** Implements `aipl eval` (via `src/vm.rs`, a 2,537-line AST interpreter), `aipl verify`, `aipl test`, `aipl compile`, and `aipl run` (via the 18 MB `aipl-run` Wasmtime launcher bundle).
2. **The self-hosted native toolchain (`aipl_src/`):** Generates standalone, statically linked Linux x86-64 ELF executables with zero libc dependencies. However, [`aipl_src/aipl.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl) currently only exposes `aipl compile` and `aipl --version`. It cannot evaluate code in-memory, run `.wasm` files directly, or provide an interactive REPL without compiling to disk and executing a native binary.

### The Problem
To make AIPL a self-sufficient developer experience and provide AI agents with a sub-second evaluation feedback loop for testing expressions and functions, we need:
- `aipl repl`: An interactive Read-Eval-Print Loop.
- `aipl eval <file.aipl>`: Direct evaluation without writing an executable to disk.
- `aipl run <module.wasm>`: Running WebAssembly files without requiring Wasmtime installed.
- `aipl test <file.aipl>`: Running module test suites directly from the self-hosted binary.

All of this must be implemented in AIPL and compiled into the native x86-64 binary.

---

## 2. Core Architectural Decision: Interpret WebAssembly, Not AIPL

There are two ways to build an interpreter in AIPL:

### Alternative A: Port `src/vm.rs` to AIPL (AST Interpreter)
Port the 2,537 lines of Rust AST interpretation to AIPL (`aipl_src/vm.aipl`).
- **Drawbacks:**
  - Creates a **tri-implementation maintenance burden** (Rust compiler/VM, self-hosted AIPL compiler, and self-hosted AIPL VM).
  - Every new language feature (e.g. value structs, pattern matching, contracts, format strings, generational pointer checks) would have to be implemented a third time in the AST interpreter.
  - Subtleties in expression typing, short-circuiting, and coercion risk drifting out of parity with compiled execution.

### Alternative B: A Pure-AIPL WebAssembly Interpreter (Recommended)
Interpret the WebAssembly bytecode emitted by [`aipl_src/codegen.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/codegen.aipl).
- **Advantages:**
  - **Zero semantic divergence:** The interpreter executes the *exact same bytes* that [`codegen.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/codegen.aipl) emits for the native x86-64 backend and Wasmtime. Every language feature (contracts, value structs, generational checks, format lowerings) is already lowered by the compiler front end.
  - **Reuses existing infrastructure:** AIPL already has a verified, 907-line Wasm decoder ([`aipl_src/native/wasm_reader.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/wasm_reader.aipl)) that decodes headers, type sections, function bodies, and instructions into structured `Instr` records.
  - **Strictly bounded scope:** The interpreter does *not* need to support arbitrary WebAssembly from clang or rustc. It only needs to support the exact 73-instruction subset emitted by `codegen.aipl` (already exhaustively enumerated in [`aipl_src/native/lower.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/lower.aipl#L62-L63)).
  - **Uncheatable test oracle:** The interpreter can be verified against Wasmtime across the entire benchmark suite, test corpus, and 50,000-case fuzzing pipeline (`tools/run_fuzz.py`).

```
                    ┌─────────────────────────┐
                    │    AIPL Source Code     │
                    └────────────┬────────────┘
                                 │ front.aipl / resolver.aipl
                                 │ checker.aipl / codegen.aipl
                                 ▼
                    ┌─────────────────────────┐
                    │   Wasm Bytecode Bytes   │
                    └────────────┬────────────┘
                                 │ wasm_reader.decode
                                 ▼
                    ┌─────────────────────────┐
                    │ Decoded Wasm IR (Instr) │
                    └──────┬───────────┬──────┘
                           │           │
     aipl_src/native/lower │           │ aipl_src/interp/
     (Batch native compile)│           │ (In-memory execution)
                           ▼           ▼
                     [Native ELF]   [aipl run / eval / repl]
```

---

## 3. WebAssembly Interpreter Specification (`aipl_src/interp/`)

The interpreter module (`aipl_src/interp/vm.aipl`) implements a stack-based virtual machine executing decoded `wasm_reader.Wasm` modules.

### 3.1. Instruction Set (The Closed 73-Opcode Subset)
The interpreter must implement only the opcodes emitted by `codegen.aipl`:
- **Constants:** `i32.const`, `i64.const`, `f64.const`, `f32.const`.
- **Variables:** `local.get`, `local.set`, `local.tee`, `global.get`, `global.set`.
- **Integer Arithmetic & Bitwise:**
  - `i32`: `add`, `sub`, `mul`, `div_s`, `div_u`, `rem_s`, `rem_u`, `and`, `or`, `xor`, `shl`, `shr_s`, `shr_u`.
  - `i64`: `add`, `sub`, `mul`, `div_s`, `div_u`, `rem_s`, `rem_u`, `and`, `or`, `xor`, `shl`, `shr_s`, `shr_u`.
- **Float Arithmetic:** `f32` and `f64` `add`, `sub`, `mul`, `div`, `sqrt`.
- **Comparisons:**
  - `i32`: `eqz`, `eq`, `ne`, `lt_s`, `lt_u`, `gt_s`, `gt_u`, `le_s`, `le_u`, `ge_s`, `ge_u`.
  - `i64`: `eqz`, `eq`, `ne`, `lt_s`, `gt_s`, `le_s`, `ge_s`.
  - `f32` / `f64`: `eq`, `ne`, `lt`, `gt`, `le`, `ge`.
- **Conversions:** `i32.wrap_i64`, `i64.extend_i32_s`, `i64.extend_i32_u`, `f64.convert_i32_s`, `f64.convert_i64_s`, `f32.convert_i32_s`, `f32.demote_f64`, `f64.promote_f32`, `i64.reinterpret_f64`, `f64.reinterpret_i64`, `i32.reinterpret_f32`, `f32.reinterpret_i32`, `i32.trunc_f32_s`, `i32.trunc_f64_s`, `i64.trunc_f64_s`.
- **Memory Operations:** `i32.load`, `i64.load`, `f32.load`, `f64.load`, `i32.load8_u`, `i32.load8_s`, `i32.load16_u`, `i32.load16_s`, `i32.store`, `i64.store`, `f32.store`, `f64.store`, `i32.store8`, `i32.store16`, `memory.size`, `memory.grow`, `memory.init`.
- **Control Flow:** `block`, `loop`, `if`, `else`, `br`, `br_if`, `return`, `call`, `call_indirect`, `drop`, `unreachable`, `end`.

### 3.2. Execution State & Zero-Allocation Dispatch Loop
In AIPL, heap allocations (`new`, `arr.new`) permanently consume space on the bump allocator. Therefore:
> [!IMPORTANT]
> The interpreter's execution loop must perform **zero dynamic allocations** per instruction.

All runtime state structures are preallocated at VM instantiation:
1. **Value Stack:** A preallocated flat array `arr i64` (e.g. 65,536 elements).
   - 32-bit integers occupy the low 32 bits.
   - Floats are stored via bitcast (`f64.reinterpret_i64` / `f32.reinterpret_i32`).
2. **Call Frame Stack:** A preallocated array `arr Frame` (e.g. 2,048 frames):
   ```lisp
   (struct Frame [
     fn_idx:i32        ;; Function index being executed
     ip:i32            ;; Current instruction pointer in Body.code
     stack_base:i32    ;; Offset in value stack where locals begin
     nlocals:i32       ;; Total parameters + declared locals
     return_sp:i32     ;; Value stack pointer to restore on return
     result_count:i32  ;; Expected results
   ])
   ```
3. **Structured Jump Table (Precomputation):**
   Before executing a function body, a one-pass precomputation parses `block`, `loop`, `if`, `else`, and `end` delimiters into a flat jump table array `arr i32`:
   - For `br` / `br_if`: Directly yields the target instruction pointer `ip` and target value stack depth.
   - For `if`: Yields the matching `else` or `end` instruction pointer on zero condition.

### 3.3. Guest Linear Memory & Bounds Checking
- Guest memory is backed by a contiguous buffer allocated on startup (initial 16 pages = 1 MiB, growable up to 1,024 pages = 64 MiB).
- Every memory load and store verifies `offset + sizeof(T) <= mem_len` before access. An out-of-bounds access raises a VM trap with source line/column coordinates (looked up via [`wasm_reader.lines_of`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/wasm_reader.aipl#L70)).

### 3.4. Host Runtime & WASI Emulation (`aipl_src/interp/wasi.aipl`)
The interpreter intercepts WASI imports and delegates them directly to AIPL's existing standard library routines:
- `fd_write`: Dispatches to `io.write_str` / `fs.write`.
- `fd_read`: Dispatches to `fs.read`.
- `path_open` / `fd_close` / `path_unlink_file`: Dispatches to `fs.open`, `fs.close`, `fs.delete`.
- `clock_time_get`: Dispatches to `sys.time` / `sys.monotonic`.
- `random_get`: Dispatches to `sys.random`.
- `proc_exit`: Terminates execution and sets the exit status.

---

## 4. The Interactive REPL Specification (`aipl_src/repl/`)

The REPL is built on top of the in-memory compiler pipeline and the Wasm interpreter.

### 4.1. Session Architecture: Incremental Compilation
Because AIPL is a compiled, statically typed language with no top-level statements outside functions, the REPL maintains a session buffer containing:
1. Accumulated imports (`(import io)`, `(import str)`, etc.).
2. Accumulated definitions (`struct`, `union`, `const`, `enum`, `fn`).
3. Accumulated persistent `let` bindings and expressions.

```lisp
;; Auto-generated wrapper for input evaluation:
(module repl_session
  ;; --- Session Imports ---
  (import io)
  (import str)
  (import buf)

  ;; --- Session Structs & Functions ---
  (struct Point [x:i32 y:i32])
  (fn add_pt [p:(ptr Point) dx:i32] -> i32 (+ (get p Point.x) dx))

  ;; --- Evaluation Entrypoint ---
  (fn __repl_eval [] -> T
    ;; Prior state bindings:
    (let p:(ptr Point) (new Point))
    (put p Point.x 10)
    ;; Current REPL input:
    (call add_pt p 5))

  (fn _start [] -> void
    (let res:T (call __repl_eval))
    (call __repl_print res)))
```

### 4.2. Return Type Inference (`T`)
AIPL functions require explicit return types. When an expression is entered into the REPL without a wrapper:
- **Strategy 1 (Static Expression Typing):** Add a helper function `checker.type_of_expr` to [`aipl_src/checker.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/checker.aipl) that determines the type of an expression AST node.
- **Strategy 2 (Probe Fallback):** If `type_of_expr` is not yet available, probe standard scalar types (`void`, `i32`, `i64`, `f64`, `bool`, `str`, `(ptr str.Bytes)`) through `front.check`. The first type that passes verification is selected.

### 4.3. Value Printing
The REPL emits a type-directed printer for `res:T`:
- `i32`: `(call io.print_int res)`
- `i64`: `(call io.print_i64 res)`
- `f64`: `(call io.print_float res)`
- `bool`: Prints `"true"` or `"false"`
- `str` / `(ptr str.Bytes)`: `(call io.println res)`
- Structs & Collections: Prints formatted address and type tag.

### 4.4. Multi-Line Input Balancing
The REPL input loop uses a tokenizer pass to count opening and closing delimiters:
- Input continues prompting (`... `) while parentheses `( ... )`, brackets `[ ... ]`, or string literals remain unclosed.
- An empty line on an open prompt cancels the pending input.

### 4.5. Built-in Commands (Colon Directives)
- `:help` — Show available commands and keybindings.
- `:type <expr>` — Parse and display the inferred type of an expression without evaluating it.
- `:defs` — List all functions, structs, and variables currently defined in the session.
- `:reset` — Clear the session environment and start a fresh module.
- `:load <file.aipl>` — Import all definitions from a local file into the session.
- `:wasm` — Print the disassembled WebAssembly instructions generated for the last input.
- `:quit` or `:q` — Exit the REPL.

---

## 5. Toolchain Subcommand Extensions (`aipl_src/aipl.aipl`)

The self-hosted compiler binary [`aipl_src/aipl.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl) will be expanded with the following command interface:

```bash
aipl repl                      # Launch the interactive AIPL REPL
aipl eval FILE.aipl [-- ARGS]  # Compile to Wasm in-memory and execute via interpreter
aipl run MODULE.wasm [-- ARGS] # Execute a standalone Wasm module via interpreter
aipl test FILE.aipl            # Execute run_all / run_tests via interpreter
aipl verify FILE.aipl          # Static verification only (already in front.aipl)
aipl compile FILE.aipl -o OUT  # Existing native/Wasm compiler
```

---

## 6. Implementation Stages

| Task | Title | Description | Est. Lines |
|---|---|---|---|
| **R1** | **Wasm Precomputation & Frame Model** | Implement `aipl_src/interp/control.aipl`: structured jump table builder (`block`, `loop`, `if`, `br`, `br_if`) and preallocated value/frame stacks. | ~350 |
| **R2** | **Integer & Logic Execution Core** | Implement `aipl_src/interp/exec_int.aipl`: evaluation of `i32`/`i64` arithmetic, bitwise operations, comparisons, and locals manipulation. | ~450 |
| **R3** | **Memory & Float Operations** | Implement `aipl_src/interp/exec_mem.aipl`: guest linear memory bounds checking, loads/stores (`load8_u`, `store32`, etc.), memory growth, and float arithmetic. | ~400 |
| **R4** | **Function Calls & Tables** | Implement `call`, `call_indirect`, return values, and global variable state. | ~300 |
| **R5** | **WASI Emulation Layer** | Implement `aipl_src/interp/wasi.aipl`: handle WASI imports (`fd_write`, `fd_read`, `clock_time_get`, `random_get`, etc.) via AIPL stdlib. | ~300 |
| **R6** | **`aipl run` Integration & Parity Testing** | Wire interpreter to `aipl_src/aipl.aipl` for `aipl run <prog.wasm>`. Verify 100% output parity with Wasmtime across all benchmarks and examples. | ~200 |
| **R7** | **In-Memory `eval` & `test`** | Connect `driver.compile_file` in memory to the interpreter. Implement `aipl eval` and `aipl test` in `aipl.aipl`. | ~250 |
| **R8** | **REPL Expression Wrapper & Type Probing** | Implement `aipl_src/repl/session.aipl`: incremental session builder, synthetic `__repl_eval` wrapper, and type resolution. | ~350 |
| **R9** | **REPL Driver & Delimiter Balancing** | Implement `aipl_src/repl/repl.aipl`: interactive terminal reader, parenthesis balancing, colon commands (`:type`, `:load`, `:reset`), and error reporting. | ~300 |
| **R10** | **Bootstrap & Regression Validation** | Rebuild stage-0 compiler with REPL/interpreter enabled; verify Stage 2 vs Stage 3 fixed point remains bit-for-bit identical. | ~50 |

**Total Estimated Implementation:** ~2,950 lines of modular AIPL.

---

## 7. Verification and Testing Strategy

1. **Differential Fuzzing Against Wasmtime:**
   Every test program in `tests/` and benchmark in `benchmarks/` must execute under both `wasmtime` and the AIPL Wasm interpreter, asserting identical stdout, stderr, and exit codes.
2. **Crash & Bounds Checking:**
   Fuzz with deliberately out-of-bounds pointer reads and stack overflow tests to ensure the interpreter safely traps with a line/col diagnostic rather than crashing with an unhandled host segfault.
3. **Stage Bootstrap Parity:**
   Because `aipl_src/aipl.aipl` compiles into the native binary, adding the interpreter must preserve the three-stage self-hosting invariant (`bootstrap/build.sh` produces bit-for-bit identical Stage 2 and Stage 3 ELF binaries).

