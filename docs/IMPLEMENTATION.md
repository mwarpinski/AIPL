# Implementation notes

How the tools in this repository implement the language: the stages of the pipeline, the `aipl` commands, the backends and what each supports, the shape of a compiled module, the self-hosted toolchain, the executables, the optimizer, and how all of it is tested. None of this is part of the language. [AIPL_SPEC.md](../AIPL_SPEC.md) defines the language, the contract the standard library keeps is in [STD_REFERENCE.md](STD_REFERENCE.md), and any other implementation has to pass `tests/conformance/` to be called AIPL. Where this document says "the spec" it means AIPL_SPEC.md, and `AIPL_SPEC.md N` names its section N.

Sections here are numbered on their own: a bare "section N" in this document is one of its own.

---

---

## 1. The pipeline

Every AIPL entry point runs the same stages in order. A failure at any stage aborts with a single diagnostic and a non-zero exit code; nothing downstream runs.

```
source.aipl
   │
   ▼
[1] Resolver   (referee/src/resolver.rs)   reads the file and every (import ...), as S-expressions (referee/src/sexpr.rs);
   │                               renames imported functions and structs to `mod.name`; one flat program
   ▼
[2] Generics   (referee/src/generics.rs)   expands every generic instance into an ordinary struct or function
   │
   ▼
[3] Parser     (referee/src/parser.rs)     each item -> typed AST (Module { name, imports, structs, functions });
   │                               every node keeps its (line, col)
   ▼
[4] Checker    (referee/src/checker.rs)    static types + contract typing; no inference across functions
   │
   ├──────────────────────────────┐
   ▼                              ▼
[5a] VM        (referee/src/vm.rs)       [5b] Wasm backend (referee/src/compiler/wasm.rs)
     tree-walking interpreter         a core wasm module (wasm-encoder)
     runs contracts and               contracts and bounds checks compiled in
     array bounds checks              │
                                      ▼
                                 [6] Native backend (aipl_src/native/, Linux x86-64, `compile --exe`)
                                      translates the wasm to an ELF executable
```

`aipl compile` adds a stage by default, between [4] and the code generator: the optimizer (`aipl_src/codegen_opt.aipl`, section 8) rewrites the checked program, which is checked again and compiled by the self-hosted code generator; `aipl compile --no-opt` takes the path drawn, through [5b].

The self-hosted toolchain mirrors every stage in AIPL: resolving and generics (`resolver.aipl`, `generics.aipl`), constants (`expand.aipl`), parsing (`parser.aipl`, into the typed tree of `ast.aipl`), checking (`checker.aipl`), and code generation (`codegen.aipl`). `front.aipl` chains the checking stages; its parser and checker report the same errors as Rust's, word for word (section 5).

---

## 2. The `aipl` commands (`referee/src/main.rs`)

| Command | Stages run | Success output |
|---|---|---|
| `aipl verify FILE` | 1-4, read as a test build reads the file (4.L): the file's own `(test ...)` bodies are checked too | `[AIPL Verifier] OK: module 'NAME' type-checks. Contracts are type-checked, not proven; ...` (nothing is proved: `req`/`ens` run on every call, in the VM and in compiled code, AIPL_SPEC.md 3); `type-checks, and so do its tests` if the file has tests (those of the files it imports are not read, as `aipl test` tests each file on its own) |
| `aipl eval FILE [--func NAME] [--listen ADDR:PORT]... [--allow-connect HOST:PORT]... [--allow-run PROGRAM]... [--max-memory SIZE] [-- ARGS...]` | 1-4, 5a | the program's own output (no banner and no result line; default `--func main`), and exits with main's status, as a compiled program does (section 6). The program's argv is `FILE ARGS...` (`std/os`); it may listen only on the addresses and ports `--listen` grants (std/net), and start only the programs `--allow-run` names (`*` for any; none by default) |
| `aipl eval -e EXPRS [--with FILE] [--wasm \| --native] [-- ARGS...]` | 1-4, then 5a (or 5b and the launcher, or 6) | the program's own output, then the last expression's value and type, `42 : i32` (any value as the formatting forms print it, `Row{n: 2} : (ptr Row)`; a function reference `<value> : T`; nothing for `void`). The expressions are the body of a function `__eval` added at the end of FILE's module (its private functions in scope; its `main` not run), or of an empty module; standard modules they name (`str.find`) are imported for them. Errors on their lines are at `-e:LINE:COL`, and a call chain leaves out the generated printer. `--wasm` and `--native` run them as the program's `_start` (refused for a FILE that defines one). Exit 0, or 1 on an error (134 for a trap under `--wasm` and `--native`, as any program) |
| `aipl compile FILE [-o out.wasm]` | 1-4, then the optimizer and the self-hosted code generator (section 8) | nothing on success; with `-v`, `[AIPL Compiler] Successfully compiled 'FILE' -> 'out.wasm' (N bytes)` on standard error |
| `aipl compile --no-opt FILE [-o out.wasm]` (with any of the options below) | 1-4, 5b | the module without the optimizer: the Rust code generator's, byte for byte the self-hosted one's (section 5), with the VM's heap layout (the optimizer shrinks the data, which moves the heap, so printed addresses differ from the VM's) |
| `aipl compile --exe [--sandbox] [--listen ADDR:PORT]... [--allow-connect HOST:PORT]... [--allow-run PROGRAM]... [--serve ADDR:PORT] [--target native\|wasm] FILE -o prog` | 1-4, 5b, then 6 for native | a standalone executable `prog`: native machine code on Linux x86-64 (section 7), else the launcher bundle (section 6). Its grants are fixed in it: it may listen where `--listen` said, and nowhere else, and start the programs `--allow-run` named (a native executable keeps each path to 256 bytes), and no others. With `--serve`, `prog` is a server giving a fresh copy of the program per connection on that address, as `aipl serve --fresh` does: natively it forks a copy of itself made once memory is ready (`aipl_src/native/fresh.aipl`), the bundle runs aipl-run's `--serve-fresh`. Until it is ended; a threaded program is refused when compiled |
| `aipl compile [--exe] --max-memory SIZE FILE -o out` | as without it | the module (or executable) with memory's maximum lowered from 2 GiB to SIZE: 1M to 2G in whole 64K pages, as `512M` (bytes, or K, M, G, binary). The module declares it, so the VM (given the same `--max-memory`), aipl-run, native executables, and any wasm host stop at the same place, the `mem.alloc` memory cannot grow to hold, with `Out of memory: the limit is 4 MiB` (for `--max-memory 4M`) and its call chain. Only lowering: below 2 GiB every address is a non-negative i32, which std and the compiler rely on |
| `aipl run [--sandbox] [--listen ADDR:PORT]... [--allow-connect HOST:PORT]... [--allow-run PROGRAM]... [--max-memory SIZE] [--no-opt] prog.aipl\|prog.wasm [ARGS...]` | the `aipl-run` launcher | runs a compiled module (section 6), or compiles a source file first and runs that; the arguments follow the file, with or without a `--`; exits with the program's status |
| `aipl serve --fresh FILE --listen ADDR:PORT [--requests N] [-- ARGS...]` | 1-4, 5b, then the command becomes `aipl-run --serve-fresh ADDR:PORT --temporary [--requests N] --name FILE prog.wasm` (one process: ending it ends the server) | a fresh instance per connection (docs/design/NET_PLAN.md, mode B): the module compiled once, then for each connection a new instance whose standard input is the connection, its standard output held until it exits (status 0: sent to the client; any other status or a trap: `500 Internal Server Error`, the program's error output on the server's), the connection then closed. An answer longer than 64 MiB is a 500 too, never part of one. FILE answers one request on its standard input, as `serve.once` does; a trap costs only its request. An address that cannot be listened on stops it with `FILE: cannot listen on ADDR:PORT: Address already in use (os error 98)` and status 1, as the executables say it. Until interrupted, or N connections. Without `--fresh` it is refused with the way to a long-running server (`aipl eval --listen`, a program that calls `serve.run`); threaded modules are refused |
| `aipl compile --self FILE [-o out.wasm]` | 1-4, 5b, then `driver.compile_file`, compiled to wasm by the Rust toolchain and run under wasmtime | compiles with the Rust toolchain and with the self-hosted one (`aipl_src/driver.aipl`: resolver, checker, code generator, as `aiplc`) and fails unless the bytes are identical (section 5); implies `--no-opt` |
| `aipl doc [MODULE \| MODULE.ITEM \| FILE.aipl] [--private]`, `aipl doc --find WORD`, `aipl doc --check FILE...` | `aipl_src/doc.aipl` compiled and run in wasmtime (the self-hosted `aipl doc` is the same code) | each public item (functions, structs with their readable fields, unions, enums, constants, interfaces) as a caller writes it, its module's own type names qualified (`fn stream.open [a:alloc.Allocator path:str] -> (result (ptr stream.Reader) str)`), then the `;;` block directly above it (a comment after code on its line, a section banner `;; =====`, or one with a blank line before the item documents nothing), then its `req`/`ens`; no MODULE lists the standard modules with their first sentence; MODULE is `./MODULE.aipl`, else the standard one; `--find` searches names and comments across the standard library, any case; `--check` exits 1 naming each public item with no comment (`cargo test` runs it over the standard library). Exit 1 on a name that does not exist, 2 on bad usage |
| `aipl lint FILE.aipl...` | `aipl_src/lint.aipl` compiled and run in wasmtime (the self-hosted `aipl lint` is the same code) | reads each file as S-expressions, before any sugar is lowered, and prints `FILE: LINE:COL: warning[RULE]: text` with the source line under it for each finding; no checker runs and no import is resolved. Rules: `unused-variable`, `unused-parameter`, `unused-function`, `unused-import`, `unreachable-code`, `leaky-in-loop`, `redundant-bool`, `empty-else`, `naming`. A name beginning `_` is exempt; `;; lint:allow RULE` (or `all`) on the line above a finding or at the end of its line silences it. Exit 0 with `lint: no findings`, 1 on any finding or an unreadable file, 2 on bad usage |
| `aipl fmt [--check] FILE.aipl...`, `aipl fmt -` | `aipl_src/fmtcmd.aipl`, run as `aipl lint` is | whitespace only: no token moves to another line. Rewrites each file in place (`--check` names the files that would change as `aipl fmt: FILE needs formatting` and exits 1; `-` reads standard input and writes standard output). Settles tabs and CRs, trailing spaces, runs of spaces, spaces inside brackets, blank-line runs, the end of the file, and a line's indentation: two columns for a body (`fn`, `let`, `cond`, `for`, `while`, `block`, `match`, ...), one for a clause or an `else`, under the first argument or one to four columns in for other forms (a line already at one of those stays), under the open bracket for a line that begins with a close one; a comment on its own stands with the code under it, at column 0, or with the code above it. Strings and comments are never touched. A file that does not read is reported (`FILE: LINE:COL: message`) and left alone. Exit 0, 1 for a file that needs formatting under `--check`, an unreadable file, or a write that failed, 2 on a command line it does not understand |
| `aipl repl` | 1-4 on each line through `aipl eval -e`'s own code (`evalexpr::run`, in the VM), in a module `repl` that holds the definitions typed so far (referee/src/repl.rs) | an expression prints `VALUE : TYPE`; a definition is kept if the module still checks; errors go to standard error and the session goes on; no prompts unless standard input is a terminal; the self-hosted `aipl` has no `repl` yet |
| `aipl test FILE\|DIR [-k TEXT] [--vm \| --native \| --all-backends]` | the test build (AIPL_SPEC.md 4.L), 1-4, 5b, run once per test | each test `ok` or `FAIL` with its output, then `N tests: P passed, F failed`; exit 1 if any failed |
| `aipl test FILE [--func run_all]` (a file with no tests) | 1-4, 5a | `[AIPL Test] All groups passed.` and exit 0; otherwise `N group(s) failed.` and exit 1 |
| `aipl watch FILE [--test] [-- ARGS...]` | `aipl eval FILE` (or `aipl test FILE`), again on each change | runs FILE, then again each time it or a file it imports changes (checked four times a second), clearing the screen first on a terminal; after each run `[ran in 0.02 s, exit 0]`; until interrupted |
| `aipl compile --tests FILE -o out.wasm` | the test build, 1-4, 5b | a module whose `_start` runs the test its first argument names (AIPL_SPEC.md 4.L) |

Errors are printed to stderr as `Error: MESSAGE`, and the exit status is 1: a file that does not read, resolve, or type-check, a file that cannot be written, a failed `aipl test`, and `aipl lint` findings. A command line `aipl` does not understand prints its usage and ends with status 2. A program that `eval`, `run`, or an executable runs ends with the program's own status, 134 for a trap (AIPL_SPEC.md 6.1); a trap in `eval` or `test` prints its call chain after the message (AIPL_SPEC.md 9). `eval` and `test` only invoke zero-argument functions. To exercise a function that takes parameters, wrap it in a zero-arg driver or write a Rust test (section 10.2).

---

## 3. The compiled module, in detail

`WasmCompiler::compile` produces a WebAssembly module with these sections, in this order: **type, import (only if the module does I/O), function, memory, export, code, data**, plus a table and element section when it uses function references. A module with compiled checks (an array index, a `req`, or an `ens`) ends with seven helper functions (`$aipl_dec`, `$aipl_oob`, `$aipl_text`, `$aipl_begin`, `$aipl_end`, `$aipl_null`, `$aipl_uaf`) that write a failed check's message (docs/design/CHECKS_PLAN.md); they are not exported. Allocation uses the threads proposal's `i32.atomic.rmw.add`, which wasmtime and every major browser accept on ordinary memory.

A **threaded module** (one that uses `thread.spawn`) differs: it imports its memory as shared (`"env" "memory"`, min 16, max 32768 pages) instead of defining it, adds a global (the address of this thread's runtime scratch cells, 64 in the main thread), a start function, a data-count section, and two compiler-generated functions after the user's: the start function, which copies the heap cursor and the string literals into memory once (guarded by an atomic flag at address 88, since every thread instantiates the module again), and the exported `wasi_thread_start(tid, record)`, which allocates the thread's 24-byte runtime scratch block, calls the worker through the function table, stores its result, and wakes `thread.join`. Its data segments are passive.

| Item | Value |
|---|---|
| Imports | only those used, in this order: from `wasi_snapshot_preview1` `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, `path_unlink_file`, `args_sizes_get`, `args_get`, `environ_sizes_get`, `environ_get`; from `wasi` `thread-spawn`; from `wasi_snapshot_preview1` `clock_time_get`, `random_get`; then AIPL's own: from `aipl_net_v1` `listen`, `accept`, `recv`, `send`, `close`, and from `aipl_sys_v1` `chmod`, `spawn`, `stream`, `read`, `write`, `close`, `wait`, `exec`, and last of all `path_rename`, `path_remove_directory`, `path_create_directory`, and `poll_oneoff` from `wasi_snapshot_preview1` (added after the others, so that no earlier import moved). A threaded module also imports its memory (`"env" "memory"`, shared). Their types come first in the type section, and every user function index is offset by the import count. A module that does no I/O has no import section and instantiates with no imports. |
| Memory | one linear memory, min 16 pages (1 MiB, same as the VM), max 32768 pages (2 GiB), exported as `"memory"` |
| Data segments | one writing the heap start at address 0 (1024, or the first 8-aligned address after the string literals); if the module has string literals, a second at address 1024 holding every distinct literal as `[len u32 LE][bytes]` |
| String literal | `i32.const <address of its bytes>`; `str` values are pointers (AIPL_SPEC.md 4.B) |
| I/O scratch | functions that do I/O, `atomic.cas/lock/unlock`, or `thread.spawn` get two extra `i32` locals; the WASI lowerings use runtime cells 64-87 for iovecs and out-parameters (AIPL_SPEC.md 7.9), or in a threaded module the same offsets in the current thread's scratch block |
| Heap cursor | the `i32` at address 0; `mem.alloc` compiles to `i32.const 0; <size>; i32.atomic.rmw.add` (the size is evaluated first, then one atomic add claims the block), followed by a check that grows memory with `memory.grow` when the cursor passes `memory.size` (no locals used) |
| Loops | `block { loop { ... } }`; a counted loop (what `for` becomes, `counted_loop`) also wraps its body in a block so `continue` falls into the step (AIPL_SPEC.md 7.10) |
| Store guard | every `mem.store*`, `put`, and `arr.set` is preceded by two checks, each an `if` that traps (`unreachable`), one if the address is in bytes 0-3 and one if it is between 64 and the heap start (the runtime block and the string literals); each function gets two extra `i32` scratch locals, the first for this check (which `ok`/`err`, `match_result`, and `arr.new` also use) and the second for the index of a bounds check |
| Bounds checks | `arr.get`/`arr.set` compare the index (unsigned) with the length at `a - 4` and on failure call `$aipl_oob`, which writes `Array index out of bounds: ...` and traps (AIPL_SPEC.md 4.E) |
| Contracts | each `req` is checked at the function's entry; with an `ens`, the body is a `block` that `return` branches to the end of, where the result is kept in a hidden local `res` and each `ens` is checked; a failure writes the message (AIPL_SPEC.md 3) through `$aipl_begin`/`$aipl_text`/`$aipl_dec`/`$aipl_end` and traps |
| Function refs | only when the module uses `ref`/`call_ref`: one extra type per distinct `call_ref` signature after the function types, a funcref table (section id 4) of every function, and an element section (id 9) filling it; `call_ref` is `call_indirect` (AIPL_SPEC.md 4.G) |
| Results and arrays | a result is its slots (multi-value); `match_result` tests the tag slot with `i32.eqz`; an array is its address and generation, its count in the header before it (AIPL_SPEC.md 4.E and 4.F) |
| Names | a `name` custom section naming every function (the generated ones `_start`, `wasi_thread_start`, `aipl.init`, and the `aipl.` check helpers), so traps can print where they happened (section 6) |
| Lines | an `aipl.lines` custom section, last: for each function, the source position (`file:line:col`) of every call and every instruction that can trap, and of every `if` (docs/design/LINES_PLAN.md has the format). Files are named by the entry file's name and each import's path (`shapes.aipl`, `vec.aipl`, `native/lower.aipl`), so the bytes do not depend on where the files are. It adds about a fifth to a module's size |
| Exports | **every** function in the flat module, under its AIPL name (`add`, `compiler.tokenize`, ...), plus `memory`; `_start` when the module has a zero-argument `main` (section 6); `wasi_thread_start` in a threaded module |
| Function types | params map `i32/bool/str` and every pointer, array, enum, union, result, and function reference to `i32`, `i64 -> i64`, `f32 -> f32`, `f64 -> f64`; a `void` return is an empty result list |
| Locals | every `let` anywhere in the body (including nested in `if`/`while`/`for`/`block`) plus every `for` variable becomes one wasm local, allocated after the params; then `res` for a function with `ens`, the two scratch locals, and the two I/O scratch locals |

Bytes 0..8 are always `00 61 73 6D 01 00 00 00` (`\0asm`, version 1). A module that compiles must also pass `wasmparser::Validator::validate_all`; the test suite enforces this.

---

## 4. Backend support matrix (as of 2026-10-19)

"Yes" means the op runs. The native backend (section 7) translates the wasm backend's output, so it supports exactly what that column supports. "Err" means the backend returns an explicit error naming the op; there are no silent defaults or no-ops in any backend. Every op the checker accepts compiles: `referee/tests/test_opcode_conformance.rs` checks each one in value and statement position, validates the wasm, and compares the self-hosted compiler's bytes. The self-hosted column is `aipl_src/codegen.aipl` (section 5).

| Ops | Checker | VM | Rust wasm backend | Self-hosted |
|---|---|---|---|---|
| `/ % divu remu ^ shl shr shru rotl rotr bitand bitor` on `i32`, and `bitnot clz ctz popcnt` | Yes | Yes | Yes | Yes |
| `+ - *` on `i32` / `i64` | Yes | Yes, error `Integer overflow in +` on overflow | Yes, trap on overflow (AIPL_SPEC.md 8.2) | Yes |
| `wrap.add wrap.sub wrap.mul` on `i32` / `i64` | Yes | Yes, wrapping | Yes (`i32.add` ...) | Yes |
| the same bitwise and division ops on `i64` | Yes | Yes, at 64 bits | Yes (`i64.*`) | Yes |
| `+ - * /`, comparisons on `f32` / `f64` | Yes | Yes | Yes (`f32.*` / `f64.*`); `%`, `divu`, `remu`, shifts, bitwise are Err | Yes (same rejections, compile error 99) |
| `eq neq lt lte gt gte` on `i32`; `eq neq` on `bool`; `and or not` | Yes | Yes | Yes | Yes |
| `ltu lteu gtu gteu` on `i32` / `i64` | Yes | Yes | Yes (`i32.lt_u` ... `i64.ge_u`) | Yes |
| `checked.add checked.sub checked.mul` on `i32` / `i64` | Yes | Yes, error `Integer overflow in checked.add` | Yes, trap on overflow (AIPL_SPEC.md 8.2) | Yes |
| `i64` literals | Yes | Yes | Yes | Yes |
| `f64` literals | Yes | Yes | Yes | Yes, with Rust's bits for every literal (section 5) |
| `f32` literals | Yes | Yes | Yes | Yes, rounded from the `f64` in integer arithmetic, with Rust's bits |
| `i64.extend_s i64.extend_u i32.wrap` | Yes | Yes | Yes | Yes |
| `f64.convert_i64_s i64.trunc_f64_s f64.reinterpret_i64 i64.reinterpret_f64` | Yes | Yes (`trunc` errors on NaN / out of range) | Yes (`trunc` traps) | Yes |
| `f32.demote f64.promote f32.convert_i32_s f32.convert_i64_s f64.convert_i32_s i32.trunc_f32_s i32.trunc_f64_s i64.trunc_f32_s f32.reinterpret_i32 i32.reinterpret_f32` | Yes | Yes (`trunc` errors on NaN / out of range) | Yes (`trunc` traps) | Yes |
| `f64.sqrt` | Yes | Yes (Rust's correctly rounded `sqrt`) | Yes | Yes |
| `mem.load8/8_s/16_u/16_s/32/64`, `mem.store8/16/32/64` | Yes | Yes | Yes | Yes |
| `u8 i8 u16 i16` fields and elements | Yes | Yes | Yes (`i32.load8_u` ... `i32.store16`) | Yes |
| `mem.alloc`, `mem.grow` | Yes | Yes | Yes | Yes |
| `atomic.add/cas/lock/unlock` | Yes | Yes, real across OS threads | Yes (wasm atomics; `lock` waits with `memory.atomic.wait32`) | Yes |
| `struct`, `new`, `get`, `put`, `sizeof` | Yes | Yes | Yes | Yes |
| `arr.new`, `arr.get`, `arr.set` | Yes | Yes, bounds-checked | Yes, bounds-checked | Yes |
| `req`, `ens` contracts | Yes | Yes, checked on every call | Yes, checked on every call (message without the position, AIPL_SPEC.md 3) | Yes |
| `inv` contracts | Yes | Yes, at entry and exit | Yes, at entry and exit | Yes |
| `ok`, `err`, `match_result` | Yes | Yes | Yes (held by value, up to 16 slots) | Yes |
| `sys.print` | Yes, `str` arguments only | Yes | Yes via WASI `fd_write` | Yes |
| `sys.exit` | Yes | returns the error `sys.exit(N) requested` | Yes via WASI `proc_exit` | Yes |
| `sys.time`, `sys.monotonic`, `sys.random` | Yes | Yes | Yes via WASI `clock_time_get` / `random_get` | Yes |
| `fs.open/read/write/close/delete/rename` | Yes | Yes, real `std::fs` | Yes via WASI | Yes |
| `args.sizes/get`, `env.sizes/get` | Yes | Yes (host-set args, process environment) | Yes via WASI `args_*` / `environ_*` | Yes |
| `proc.spawn/stream/read/write/close/wait/exec` | Yes, real processes, granted by `--allow-run` | Yes, the same | Yes via `aipl_sys_v1`, which aipl-run provides (a stock WASI host does not) | Yes |
| `fs.chmod` | Yes, `std::fs::set_permissions` | Yes, the same | Yes via `aipl_sys_v1`, which aipl-run provides (a stock WASI host does not) | Yes |
| `net.listen/accept/recv/send/close/connect` | Yes | Yes, real `std::net`, granted by `--listen` and `--allow-connect` |
| `net.connect_host`, `net.tls_connect` | Yes (the system's resolver; rustls) | Yes | Yes via `aipl_net_v1` (aipl-run) | `-3`: std/net then starts `aipl-net`, which resolves, connects, and does TLS for the executable (see below) | Yes via `aipl_net_v1`, which aipl-run provides (a stock WASI host does not) | Yes |
| `ref`, `call_ref`, `(fn [..] -> r)` types | Yes | Yes | Yes (funcref table, `call_indirect`) | Yes |
| `const`, `enum`, `enum.cast`, `enum.ord` | Yes | Yes | Yes (literals and `i32`; no code of their own) | Yes (erased by `consts.aipl`) |
| `union`, `make`, `match` | Yes | Yes | Yes (held by value, up to 16 slots; `match` is an `if` chain ending in `unreachable`) | Yes |
| `return`, `break`, `continue`, `cond` | Yes | Yes | Yes (`return`, `br`; `cond` is nested `if`) | Yes |
| `thread.spawn / thread.join` | Yes | Yes, real `std::thread`; the worker is a `(fn [i32] -> i32)` reference | Yes, as a threaded module (AIPL_SPEC.md 4.D): needs a host that provides `wasi.thread-spawn` | Yes |
| `str` literals, `str.len`, `str.ptr` | Yes | Yes | Yes (interned data segment, pointer identity) | Yes |
| `(import ...)` | resolved before checking | | | resolved first by `resolver.aipl` (`driver.aipl` chains the two); `compile_module` itself takes one import-free module |

Rule of thumb for code generators: whatever `aipl verify` accepts runs in the VM and compiles. Compiled I/O needs a WASI host with a preopened directory (section 10.5), and threads need a wasi-threads host (AIPL's runner). Array bounds checks and `req`/`ens` contracts run in every backend.

---

## 5. The self-hosted toolchain (`aipl_src/codegen.aipl`)

`codegen.compile_module [src_ptr:i32 src_len:i32] -> i32` tokenizes and parses AIPL source (via `compiler.tokenize` / `compiler.parse_ast`) and emits a complete wasm module. It stores the output pointer in cell 60 and returns the byte length, or `-1` with a nonzero compile error code in cell 4 (the first error encountered; later ones are usually consequences). For everything it accepts, the output is required to be **byte-identical** to `WasmCompiler::compile`. `referee/tests/test_selfhost.rs` enforces this on over 40 programs, including `memory.aipl`, `compiler.aipl`, the standard library, and codegen.aipl itself (flattened with its imports by the Rust resolver first, since `compile_module` does not resolve imports). `aipl compile --self` checks the same thing for any file and, on a mismatch, reports the first differing byte, the section (and code-section function) it falls in, and a hex window of each side.

It infers each expression's static type the way `expr_type` in `referee/src/compiler/wasm.rs` does (`node_type` / `group_type`) and selects `i32.*` / `i64.*` / `f32.*` / `f64.*` instructions, `if` and `match_result` block types, struct field and array element load/store widths, and alignment from it. It runs in the VM today; compiled to wasm it also compiles itself (section 10.6). Limits that differ from the Rust backend:

- **One module, no imports.** `compile_module` itself takes a single import-free source. Imports are flattened first by `aipl_src/resolver.aipl` (AIPL_SPEC.md 11), which `aipl compile --self` runs in the VM; `aipl_src/driver.aipl` chains the two (`driver.compile_file`), and its `main`/`_start` make it a command: compiled to wasm, `wasmtime run --dir . --env AIPL_PATH aiplc.wasm IN.aipl OUT.wasm` compiles a multi-file program with no Rust involved (standard library at `$AIPL_STD`, else `share/aipl/std/` beside the executable's folder, else `aipl_src/std/` below the working directory: `aipl_src/stdpath.aipl`); `aipl compile --exe aipl_src/driver.aipl -o aiplc` makes the same command an executable (`./aiplc IN.aipl OUT.wasm`); exit status 0, 1 on a resolve, type, compile, or write error, 2 on bad usage. The driver type-checks before compiling (`front.aipl`), so it rejects every program `aipl verify` rejects, with the same message; it reports the error as `path: line:col: message` in the file the error is in, mapping the flat program back through the resolver's and the generics pass's re-printing (`origins.aipl`; an error inside a generic template is reported at the template). In the VM: `aipl eval aipl_src/driver.aipl -- IN.aipl OUT.wasm`. The byte-parity tests in `referee/tests/test_selfhost.rs` still flatten with the Rust resolver and `referee/src/printer.rs` (`referee/tests/test_printer.rs` checks that round trip), which keeps them independent of the AIPL resolver. `aipl_src/aipl.aipl` adds the native backend to the driver: built natively, it is the stage-0 compiler, `aipl compile [--exe [--sandbox]] FILE.aipl -o OUT`, which builds the next version of itself with no Rust (bootstrap/README.md). A qualified call such as `(call util.f)` resolves only if a function with that exact name is defined in the source given.
- **Float literals get Rust's bits.** `compile_module` converts an `f64` literal with `std/float`'s `from_decimal` (docs/STD_REFERENCE.md), which rounds to the nearest double, ties to even, as Rust's `parse::<f64>` does: one IEEE multiplication or division when the digits form an integer of at most 2^53 and the power of ten is at most 22 either way, and exact big-integer arithmetic (`std/bigint`) otherwise, subnormals, infinities, and halfway cases included. Both tokenizers read the same literals (a leading `+`, exponents such as `1.5e3`).

Buffers are sized from the input: tokens exactly (`compiler.count_tokens` first, `(sizeof compiler.Token)` each), AST `(sizeof compiler.Node) * (tokens + 2)`, and output, section scratch, and function scratch `4 * src_len + 64 KiB` each. Memory is grown with `mem.grow` as needed, so a compile works within the 32768-page limit shared by both backends. Compiling the whole self-hosted toolchain (driver, resolver, checker, codegen, and the standard library it uses), checking included, takes about 47 MB of process memory in the native `aiplc` (2026-10-06).

| Compile error (cell 4) | Meaning |
|---|---|
| 90 | `mem.grow` refused: the compile needs more than 32768 pages |
| 91 | output or a function body exceeded `4 * src_len + 64 KiB` |
| 92, 93 | not given since 2026-10-26: the tables of functions, parameters, and locals grow, as the Rust code generator's vectors do (they were 8192 functions, 16 parameters, 1024 locals) |
| 94 | more than 16 scalars in a value struct (which the checker refuses first); the tables of structs, unions, variants, fields, and `call_ref` signatures grow since 2026-10-26 |
| 95 | a struct field, array element, or `sizeof` type that cannot be stored in memory (`void`, a result) |
| 96 | unknown struct or field in `new`/`get`/`put`/`sizeof` |
| 98 | a type the backend cannot lower (an unknown type name, or `void` where a value is needed) |
| 99 | an operator applied to a type with no wasm instruction for it (e.g. `%` on `f64`, `+` on `str`) |
| 100 | no longer given (2026-10-19): the label stack grows as wasm.rs's does; before, past 255 nested blocks it crashed instead of reporting this |
| 101 | `break` or `continue` with no enclosing loop (the Rust checker rejects this first) |
| 768 | the string literals and the heap start do not fit in the first 1 MiB of memory, the limit both backends share |
| 971 | a symbol that is not a local or parameter |
| 974 | `Name.member` naming a member its enum does not have (`consts.aipl`; the Rust resolver reports it first) |
| 975 | `make` or a `match` arm naming a variant its union does not have (the Rust checker reports it first) |
| 987 | a form whose head is not a recognised keyword |
| 999 | an empty expression where one is required |
| 1452 | call to an undefined function; cells 44/48 hold the callee name's source offset and length |

---

## 6. Standalone executables and the `aipl-run` launcher

A compiled module runs under `aipl-run` (`referee/src/bin/aipl_run.rs`), the toolchain's native launcher: wasm cannot start itself, so a small Rust program embeds wasmtime, loads the module (compiled by Cranelift at start-up, about 10 ms for a small program), and connects it to the operating system. It contains no compiler logic. On Linux x86-64, `aipl compile --exe` writes native executables instead (section 7), which need no launcher.

- **Entry point and exit status.** A module with a zero-argument `main` and no `_start` of its own gets an exported `_start` that calls it, so the module is a WASI command. `main` gives the exit status, the same in every backend (`aipl eval` exits with it too), and returns one of three types; the checker refuses any other:
  - `i32`: the status itself, 0 for success and 1-125 for a failure (`_start` calls `proc_exit` with a status that is not 0).
  - `(result i32 str)`: `(ok:str 0)` (or another status), or `(err:i32 "why")`, whose message goes to standard error, then a newline, and the status is 1 (`_start` reads the result itself; `aipl eval` does the same with main's value).
  - `void`: status 0.

  `sys.exit` ends the program with its status from anywhere. A status must be 0-125: as under wasmtime, any other value (including a negative one) is an error reported with status 134, since shells reserve 126 and up. A program's output is what it prints or writes; main's value is only the status, so a program reporting a number prints it (`(println "{n}")`) and returns 0. A trap (a failed contract or bounds check included) prints the program as invoked and the reason, then its call chain, the functions it happened in, innermost first, and exits with 134:

  ```
  ./prog: wasm trap: integer divide by zero
    at math.div (math.aipl:4:5)
    at main (prog.aipl:9:22)
  ```

  Each frame names its function and where in the source it was: the expression that trapped for the innermost frame (the operation, the `arr.get` of a failed bounds check, the condition of a failed `req` or `ens`), and the call for each frame below it. In an optimized build (section 8) a function inlined into its caller has no frame: a trap inside it is reported at the call, in the caller's frame (`at main (prog.aipl:9:22)` above, had `math.div` been inlined). A frame with no recorded position shows only its name, such as the innermost frame of a stack overflow, which traps on entry. The chain comes from the module's name section and the positions from its line table (section 3), and it leaves out the generated functions (`_start`, `wasi_thread_start`, and the `aipl.` check helpers) and any without a name; it shows at most 32 frames, then `  ... N more`. A trap in a spawned thread shows that thread's chain. The exit-status error above is not a trap: one line. `AIPL_BACKTRACE=1` makes `aipl-run` print wasmtime's full report instead. Native executables print the same lines, walking their own frame pointers. (`main`'s return value is not the exit status: many programs return data.)
- **What the program sees.** The real stdin, stdout, and stderr; the command line (argv[0] is the program); the environment; the working directory as preopened fd 3 and `/` as fd 4, so relative and absolute paths both work (AIPL_SPEC.md 4.C); and, for a threaded module, the wasi-threads `thread-spawn` import (AIPL_SPEC.md 4.D). `--sandbox` grants only the working directory.
- **`aipl compile --exe --target wasm FILE -o prog`** (the default on platforms without a native backend) writes the launcher followed by the module, the grants (one a line: `--listen`'s `ADDR:PORT`, or `run:PROGRAM` for each `--allow-run`), and a 20-byte trailer: flags (`u32` LE, bit 0 = sandbox, bit 1 = `--serve`, whose address is then the grants' first line), the grants' length and the module's (`u32` LE each), and the magic `AIPLEXE2`. The launcher finds a module appended to its own file and runs it; without one it runs a `.wasm` named on its command line (`aipl-run [--sandbox] [--listen ADDR:PORT]... [--allow-run PROGRAM]... prog.wasm ARGS...`). The result is one file with no other dependency, about 18 MB in a release build (the size is wasmtime), built for the host's OS and CPU. The `.wasm` remains the portable form.
- The launcher is found as `aipl-run` next to the `aipl` binary, or at `$AIPL_RUNNER`. Writing the bundle is done by the Rust CLI only because WASI cannot set a file's executable bit; the format is plain bytes.

---

## 7. Native executables (Linux x86-64 and AArch64)

`aipl compile --exe FILE -o prog` on Linux x86-64 (or `--target native`) writes a native executable: an ELF file of machine code, statically linked, with no libc, launcher, or runtime (`aipl_src/native/`, designed in `docs/design/NATIVE_BACKEND_PLAN.md`). It is made from the same wasm module (`aipl_src/native/native.aipl` translates wasm to x86-64; the CLI runs it in its embedded wasmtime and sets the file's execute bit), so its behaviour is the wasm module's, and every test program is checked to match `aipl-run`'s output, error output, exit status, and files byte for byte (`referee/tests/test_native.rs`).

- **Size and speed.** `word_count` is about 94 KB (250 KB without the optimizer); the AIPL compiler itself, with its native backend, about 5.7 MB (the launcher build is 18 MB). Start-up is a few milliseconds (no engine to load or compile). The translator is a single-pass compiler that tracks where each value is (a register, a constant, a local) and emits code only when a value is used (docs/design/NATIVE_SPEED_PLAN.md): the natively built compiler compiles itself in 0.46 s against 0.53 s under wasmtime (which includes Cranelift's compile), and the eight benchmarks run in 0.8 to 1.9 times wasmtime's time, 1.9 on nbody, whose float-heavy loop has more values than registers (docs/BENCHMARKS.md). Memory accesses are checked by guard pages: an access outside the memory faults, and the fault becomes wasm's out-of-bounds trap with the same message and call chain. A function returns one result in `rax`; several (a value struct, AIPL_SPEC.md 4.E) go through 8 slots in the thread's block after its stack limit, stored by the function just before it returns and loaded by the caller right after the call.
- **Same behaviour as section 6:** `_start`/`main`, exit statuses (0-125, otherwise an error with 134), traps with their call chain and status 134 (`./prog: wasm trap: ...` and `  at f` lines; WASI functions given a bad pointer report wasmtime's `Pointer out of bounds: Region { start: S, len: L }`), the command line, environment, stdio, fd 3 = working directory, fd 4 = `/` (not with `--sandbox`), threads (one OS thread each), clocks, and randomness.
- **File access.** Paths resolve beneath their directory descriptor with `openat2(RESOLVE_BENEATH)`, so the kernel refuses `..` escapes, absolute paths under fd 3, and symlinks leading out, as wasmtime's preopens do. File descriptors are numbered as under wasmtime (the lowest free). `openat2` needs Linux 5.6 or newer, and Rosetta 2 and some container profiles lack it (the call fails with ENOSYS, or with EPERM from a seccomp filter). A program built without `--sandbox` then opens with plain `openat`, with the same flags and mode: it is not confined to its directory, which costs it nothing, since `/` is its fd 4 and any absolute path opens anyway (it follows `..` and symlinks out where `aipl-run` refuses). A program built with `--sandbox` probes `openat2` at start and, without it, writes `this program was built with --sandbox, which needs the openat2 system call (Linux 5.6 or newer, and not Rosetta 2)` to standard error and exits 1. Everything else a native executable does works on any kernel since 3.17. Tested by refusing the call with a seccomp filter (`tools/without_openat2.py`, `referee/tests/test_openat_fallback.rs`, and `referee/tests/test_native.rs` run that way by CI); not yet run under Rosetta 2 itself.
- **Processes.** `aipl_sys_v1`'s `spawn`, `stream`, `read`, `write`, `close`, `wait`, and `exec` become Linux system calls (`aipl_src/native/procs.aipl`), with referee/src/sys.rs's behaviour: the programs a program may start are fixed in the executable when it is compiled (`--allow-run`, at most 256 bytes each), a pipe's handle is its descriptor and a process's a slot of 64 (anything else is refused with `-8`), and a program that cannot start is told to the parent by a pipe that closes on exec, so a missing program is `-44` as it is under aipl-run.
- **Sockets.** `aipl_net_v1` becomes Linux system calls (`aipl_src/native/sockets.aipl`), with referee/src/net.rs's behaviour: the grants are fixed in the executable when it is compiled (`--listen`, `--allow-connect`), a handle is the Linux descriptor of a socket the program opened (anything else is refused with `-1`), and `send` uses `MSG_NOSIGNAL`.
- **Serving a fresh copy per connection** (`--serve ADDR:PORT`, `aipl_src/native/fresh.aipl`). Start-up listens on the address once linear memory is ready, before the program's start function and `_start`; for each connection it forks (`fork`, after `pipe2`), and the child takes the connection as its standard input and the pipe as its standard output, closes the host's other descriptors, and goes on with start-up, so the program runs from the beginning in a copy of memory that only its writes change. The host reads the pipe to its end (64 MiB kept), waits for the child, and sends its output if it exited with status 0, else aipl-run's plain 500; then shuts the connection down. The child's standard error is the host's, so a trap is logged there. One connection at a time.
- **Platforms.** Linux x86-64 only; elsewhere `--exe` writes the launcher bundle (section 6), and `--target native` is an error. `--target wasm` asks for the bundle on Linux too.

---

## 8. The optimizer (`aipl_src/codegen_opt.aipl`)

`aipl compile` builds through the optimizer unless `--no-opt` is given, for every output (`.wasm`, `--exe`, `--tests`, `--max-memory`). It is a source-to-source pass over the checked program after formats are lowered (docs/design/OPT_PLAN.md): constant folding, algebraic identities, and power-of-two strength reduction (`i32` and `i64` wrap and shift counts are masked, so these are exact); inlining of small functions (an argument that is not an atom, or a pure expression used once, is first bound to a `let` with a fresh name, in the call's order, so each argument is evaluated once, where the call evaluated it); propagation of immutable locals; and removal of the functions `main` cannot reach. Contracts are left as written, since a failed one prints its text. `AIPL_OPT_DUMP=FILE aipl compile ...` writes the optimizer's output text to FILE, for reading what it did.

- **Same behaviour.** An optimized program prints the same output, writes the same files, and exits with the same status as the unoptimized one, and a trap gives the same message. tools/opt_diff.py checks this in CI on every program in the repository and on 500 generated ones (tools/run_fuzz.py's).
- **What differs.** A function inlined into its caller has no frame of its own: a trap inside it is reported at the call, in the caller's frame (AIPL_SPEC.md 9). The data is smaller, so the heap starts lower and printed addresses (`(ptr T) @ N`, a failed contract's pointer arguments) differ from the VM's and from an unoptimized build's. The code is smaller and faster: spectralnorm runs in about 0.6 of the time, knucleotide 0.85, nothing slower (docs/design/OPT_PLAN.md).
- **Its output is checked.** What the optimizer makes is type-checked before code generation; a program that does not check is a compile error naming the optimizer and `--no-opt`, never wrong code. A failure of the optimized build on a program the checks passed is a toolchain bug; `--no-opt` builds without it.
- **The two commands agree.** The Rust command checks the program with its own front end (so every error reads as without the optimizer), then runs `driver.compile_file_opt` (`aipl_src/driver.aipl`), compiled to wasm, under wasmtime; the self-hosted command (`aipl_src/aipl.aipl`) calls it directly. Both write the same bytes, optimized or not; the bootstrap's stage 2 is an optimized build of the compiler, and stage 3 is identical to it.
- **The library.** What `aipl_src/` holds (the standard library and the toolchain's sources: the driver, the native backend, `aipl doc`) is built into the `aipl` binary (`referee/build.rs`, `referee/src/library.rs`). Run from its own checkout's `target/`, `aipl` reads that checkout's `aipl_src/`, so an edit there is seen at once; anywhere else (copied, installed, unpacked from a kit) it writes its built-in copy once to the cache directory below, as `library-HASH/`, and reads that, so the binary needs nothing beside it. `$AIPL_STD` replaces the standard library for the programs it compiles and runs, not for its own modules. referee/tests/test_portable.rs runs a copied `aipl`.
- **Module cache.** The toolchain modules the Rust command runs under wasmtime (the driver for the optimizer, `native/native` for `--exe`) are compiled to machine code once and kept in `$AIPL_CACHE`, else `$XDG_CACHE_HOME/aipl`, else `~/.cache/aipl`, keyed by the Rust sources and every file below `aipl_src/`: a compile takes about 0.08 s, the first after a change about 0.5 s. An empty `AIPL_CACHE` turns the cache off; an entry wasmtime refuses is compiled again.

---

## 9. How the operations lower to WebAssembly

AIPL code maps 1-to-1 to WebAssembly binary opcodes:
- `(mem.load32 ptr)` -> `i32.load (MemArg { offset: 0, align: 2 })`
- `(mem.store32 ptr val)` -> `i32.store (MemArg { offset: 0, align: 2 })`
- `(shl a b)` -> `i32.shl`
- `(shr a b)` -> `i32.shr_s`
- `(bitand a b)` -> `i32.and`
- `(bitor a b)` -> `i32.or`
- `(rotl a b)` -> `i32.rotl`, `(rotr a b)` -> `i32.rotr` (the count modulo 32; `i64.rotl`, `i64.rotr` on `i64`, modulo 64)
- `(clz a)`, `(ctz a)`, `(popcnt a)` -> `i32.clz`, `i32.ctz`, `i32.popcnt` (the width for `clz` and `ctz` of 0; `i64.*` on `i64`, with an `i64` result)
- `(bitnot a)` -> `i32.xor` with -1 (`i64.xor` with -1 on `i64`)

### 9.1 Checked arithmetic

The wasm lowering (`compile_checked` in `referee/src/compiler/wasm.rs`, `codegen.aipl` alike): for `i32`, both operands are widened to `i64`, the exact result is computed there, and it traps unless it lies in `[-2^31, 2^31)`; for `i64`, addition and subtraction test the sign bits (`(a ^ r) & (b ^ r) < 0` for `+`, `(a ^ b) & (a ^ r) < 0` for `-`) and multiplication checks `r / a == b` when `a` is not 0 (and the `-1 * MIN` case through the same division). The trap is an `i32.trunc_f32_s` of `3e9`, which every host reports as integer overflow (a division of `MIN` by `-1` traps the same way, but a function that holds one keeps more on its stack: its recursion is half as deep under wasmtime). `x + c` and `x - c` with a literal `c` are tested by one comparison of `x` instead (`x > MAX - c` for a positive `c`). `referee/tests/test_differential.rs` runs every pair of 12 edge values per width through the VM and wasmtime and compares both with Rust's `checked_*`.

### 9.2 Control flow

Lowering (wasm and the self-hosted compiler): `return` is the `return` instruction, or in a function with an `ens` a branch to the end of the body's block, where the `ens` checks run (section 3). Loops are `block { loop { ... } }`; `break` branches to the outer block. A `while` body's `continue` branches to the loop header. A `loop` wraps its body in one more block, and `continue` branches to that block's end, which falls into the step. `(and a b)` is `a; if (result i32) b else i32.const 0 end` and `(or a b)` is `a; if (result i32) i32.const 1 else b end`. Branch depths count the enclosing `if`s (including those of `and`/`or`) and `match_result` arms.

### 9.3 Type-directed code generation

The wasm backend selects instructions from the static operand type (`i32` / `i64` / `f32` / `f64`), and an `if` whose branches are `i64` or `f64` gets a matching block result type. `f64` arithmetic (`+ - * /`) and comparisons therefore compile and validate. `%`, `divu`, `remu`, shifts, and bitwise ops on floats are rejected at compile time with `Wasm Codegen: <op> is not supported for operands of type f64`.

**Status:** the wrapping rules above for both widths are implemented in `referee/src/vm.rs`, covered by `referee/tests/test_i64.rs` (VM result plus wasm validation), and proven equal across backends by `referee/tests/test_differential.rs`, which executes every case in this section in both the VM and wasmtime and asserts identical results. When the two disagree, wasm is right and the VM is fixed (section 10.4).

---

## 10. Testing

There are three layers of tests. Each one must fail loudly on fabricated work; a test that only asserts "returned a number" or "produced more than N bytes" is not acceptable in this repository (see `docs/history/AUDIT_2026-10.md`, item U6).

### 10.1 AIPL-native self-tests (`aipl test`)

Each real module in `aipl_src/` exposes a zero-arg `run_<module>_tests` function that returns an `i32` count of passing sub-tests (or `1` for a single pass / `0` for fail). The suite entry point `aipl_src/test_suite.aipl` imports every verified module, calls each runner, compares against the expected count, prints one `[PASS]`/`[FAIL]` line per group with `sys.print`, and returns the number of failing groups. The Rust `test` subcommand turns that into the exit code.

```lisp
;; aipl_src/test_suite.aipl (abridged)
(module test_suite
  (import compiler)
  (import memory)
  (import io)

  (fn report [label:str result:i32 expected:i32] -> i32
    (if (eq result expected)
        (block (call io.write_str 1 "[PASS] ") (call io.println label) 0)
        (block (call io.write_str 1 "[FAIL] ") (call io.println label) 1)))

  (fn run_all [] -> i32
    (let failures:i32 0)
    (set! failures (+ failures (call report "compiler: tokenizer (2 tests)" (call compiler.run_tokenizer_tests) 2)))
    (set! failures (+ failures (call report "memory: allocator + arena"     (call memory.run_memory_tests)     1)))
    failures))
```

Pattern for a module's own runner: perform real work, then assert on the **content** of the result, not its shape.

```lisp
;; aipl_src/memory.aipl
(fn run_memory_tests [] -> i32
  (let p1:i32 (call aipl_heap_alloc 64))
  (let p2:i32 (call aipl_heap_alloc 128))
  (let arena:i32 (call arena_create 1024))
  (let a1:i32 (call arena_alloc arena 32))
  (let a2:i32 (call arena_alloc arena 64))
  (let _r:i32 (call arena_reset arena))
  (let a3:i32 (call arena_alloc arena 32))
  (if (and (gt p2 p1) (and (gt a2 a1) (eq a3 a1))) 1 0))
```

Expected terminal output (30 groups; abridged):
```
[AIPL Test] Running 'run_all' from 'aipl_src/test_suite.aipl'...

[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
...
[PASS] std/bigint: arbitrary-precision integers (13 tests)
...
[PASS] consts: constants and enums erased for codegen (5 tests)
[PASS] thread_sync: 4 threads x 1000 atomic adds = 4000

[AIPL Test] All groups passed.
```

`aipl test` runs the suite in the VM; it also compiles to wasm.

Rules: import a module into `test_suite.aipl` only once its runner is verified to do real work. `thread_sync.aipl` is imported like every other module: `thread.spawn` takes a function reference, which survives the resolver's renaming (P10).

### 10.2 Rust integration tests (`cargo test`, `referee/tests/*.rs`)

Rust tests drive the library crate `aipl_core` directly and are the only way to call AIPL functions with arguments. The canonical shape exercises **every** stage and asserts concrete values:

```rust
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmparser::Validator;

#[test]
fn fib_runs_and_compiles() {
    let src = r#"
    (module compute
      (fn fib [n:i32] -> i32
        (if (lte n 1) n
            (+ (call fib (- n 1)) (call fib (- n 2))))))
    "#;
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");

    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("fib", vec![Value::Int(7)]).unwrap(), Value::Int(13));

    let wasm = WasmCompiler::compile(&module).expect("compile");
    assert!(wasm.starts_with(&[0x00, 0x61, 0x73, 0x6D]));
    Validator::new().validate_all(&wasm).expect("wasm must validate");
}
```

To assert a diagnostic, match on the exact `L:C:` prefix so a regression in position tracking fails the test:

```rust
let err = Parser::parse("(module m\n  (fn f [] -> i32 42)").unwrap_err();
assert!(err.starts_with("2:21:"), "got {err}");
```

To assert that a backend **rejects** something rather than faking it:

```rust
let module = Parser::parse("(module m (fn f [] -> void (sys.print 5)))").unwrap();
let err = WasmCompiler::compile(&module).unwrap_err();
assert!(err.contains("sys.print supports str arguments only in the wasm backend"));
```

Files today (76 files, 557 tests on Linux as of 2026-11-01):

- `referee/tests/test_all.rs` (pipeline smoke)
- `referee/tests/test_v2.rs` (memory, atomics across real threads, real file I/O, results, imports)
- `referee/tests/test_diagnostics.rs` (exact `L:C:` prefixes)
- `referee/tests/test_i64.rs` (64-bit type, VM plus wasm validation)
- `referee/tests/test_memory_layout.rs` (reserved-block enforcement in both backends)
- `referee/tests/test_opcode_conformance.rs` (10.3)
- `referee/tests/test_differential.rs` (10.4)
- `referee/tests/test_wasi.rs` (10.5)
- `referee/tests/test_selfhost.rs` (10.6)
- `referee/tests/test_doc_examples.rs` (10.7)
- `referee/tests/test_pointers.rs` (strict pointer/array typing, VM/wasm agreement, struct namespacing across imports)
- `referee/tests/test_std.rs` (every eligible standard-library function in both backends under WASI, plus exact printed output)
- `referee/tests/test_printer.rs` (source round trip of every repository program)
- `referee/tests/test_refs.rs` (function references in both backends, signature checks, refs across imports)
- `referee/tests/test_control_flow.rs` (return/break/continue/cond, short-circuit `and`/`or`, loop bounds evaluated once, in both backends; checker rejections)
- `referee/tests/test_threads.rs` (threaded modules under a wasi-threads host against the VM)
- `referee/tests/test_generics.rs` (template expansion in both backends, Rust/AIPL parity, errors)
- `referee/tests/test_visibility.rs` (what a module gives and keeps, both resolvers, message for message)
- `referee/tests/test_interfaces.rs` (interfaces, implementations, where clauses, and templates checked once, in both backends and both toolchains, message for message)
- `referee/tests/test_resolver_aipl.rs` (the AIPL resolver against the Rust one, subdirectory imports)
- `referee/tests/test_runner.rs` (the `aipl-run` launcher and `aipl compile --exe` executables)
- `referee/tests/test_benchmarks.rs` (every program in `benchmarks/`, as wasm and natively, against its `expected.txt`)
- `referee/tests/test_consts_enums.rs` (constants and enums in the VM, wasm, and natively, and every checker rule)
- `referee/tests/test_sum_types.rs` (unions and `match` in every backend, and the checker's rules)
- `referee/tests/test_values.rs` (value structs, and unions, results, and `Option` by value, in every backend and both toolchains, with every checker rule)
- `referee/tests/test_checker_aipl.rs` (the AIPL tokenizer, parser, printer, and checker against the Rust ones, message for message, run compiled to wasm)
- `referee/tests/test_heap.rs` (`std/heap`'s failure checks: the same message from the VM, `aipl-run`, and native executables)
- `referee/tests/test_par.rs` (`examples/spectralnorm_par.aipl`, through `std/par`, prints the sequential benchmark's result on 1, 3, and 8 threads in the VM, under aipl-run, and natively)
- `referee/tests/test_generations.rs` (use after free through a copy, after reuse, of an array, after an arena or fast arena reset, and through an allocator, and versioned loops failing exactly where unversioned ones would, with the same output and call chain in the VM, under aipl-run, and natively)
- `referee/tests/test_language_gaps.rs` (`f32` values, conversions, and traps at wasm's bounds, `f32` literal rounding in both toolchains, 8- and 16-bit storage types, `inv` at entry and exit, and constant expressions, each in the VM, under aipl-run, and natively, with the self-hosted toolchain's bytes the Rust one's)
- `referee/tests/test_formats.rs` (the formatting forms alike in the VM, under aipl-run, and natively, and in both toolchains' bytes; every formatting error, message for message, among them the strings a placeholder refuses; strings written `\"...\"` inside a placeholder; `std/show` against Python on generated values and formats)
- `referee/tests/test_early_returns.rs` (branches that return, break, or continue in value position; `try`, `try_opt`, `try_or`, and `defer` in every backend and both toolchains, with every checker rule)
- `referee/tests/test_text.rs` (`std/text` and `std/str`'s search, split, lines, replace, and trim against Python on generated inputs, in the VM, under aipl-run, and natively, and in both toolchains' bytes)
- `referee/tests/test_stream.rs` (a program reading standard input a line at a time and writing through a buffered writer prints what Rust's `str::lines` gives, in the VM, under aipl-run, and natively, and in both toolchains' bytes; `format_to` on a writer that is not a variable, one message in both)
- `referee/tests/test_repl.rs` (`aipl repl`: values and types, definitions kept and replaced, forms over several lines, errors that leave the session going)
- `referee/tests/test_fmt.rs` (`aipl fmt`: white space settled, indentation by form, an accepted indentation staying, comments and strings untouched, a second run changing nothing and no token moving over real files, the repository's own programs formatted (the ones with `@reject` or `@syntax` excepted), `--check` and in-place writes, a file that does not read; the self-hosted `aipl fmt` agreeing with the Rust command is in `referee/tests/test_bootstrap.rs`)
- `referee/tests/test_lint.rs` (`aipl lint`: each rule's findings with their lines and columns, what it leaves alone, `lint:allow`, and the exit statuses; the self-hosted `aipl lint` printing the Rust command's text over every file of `aipl_src/`, `examples/`, and `benchmarks/`, in referee/tests/test_bootstrap.rs)
- `referee/tests/test_doc.rs` (`aipl doc` on a module with every kind of item, one item, a search, and errors; every public item of the standard library documented)
- `referee/tests/test_eval_expr.rs` (`aipl eval -e`: values with their types in the VM, under aipl-run, and natively, `--with` a file's private functions, standard modules imported, errors at `-e:LINE:COL`; the self-hosted `aipl eval` printing what the Rust command prints natively, for each of those, a trap's call chain, and `aipl eval FILE` as `aipl run`)
- `referee/tests/test_dbg.rs` (`(dbg e)` for every kind of value, in an imported module, a generic, nested, and in a `try`, alike in the VM, under aipl-run, and natively, and in both toolchains' bytes; every mistake one message in both)
- `referee/tests/test_testing.rs` (`aipl test` on a file of passing, failing, and trapping tests, alike under aipl-run, in the VM, natively, and with `--all-backends`; `-k`; ordinary builds leave tests out, and `aipl verify` reads them; the self-hosted `aipl compile --tests` gives the Rust toolchain's bytes and each mistake's message; the self-hosted `aipl test` prints the Rust command's lines, failures, trap, counts, and statuses for a file, a directory, and `-k`, leaves no temporary build, and refuses what needs the VM)
- `referee/tests/test_watch.rs` (`aipl watch` running a file again when a file it imports changes, and a file's tests with `--test`; the self-hosted `aipl watch` doing the same, natively and with the program's arguments, and watching a program that does not compile)
- `referee/tests/test_sugar.rs` (`when`, `for`, and variadic `and`/`or` alike in every backend and both toolchains' bytes, with each mistake one message in both and a test's body read only in a test build)
- `referee/tests/test_csv.rs` (std/csv against Python's csv on generated texts and rows, and its API, in every backend)
- `referee/tests/test_openat_fallback.rs` (native executables with the openat2 system call refused: the fallback to openat, and the --sandbox refusal)
- `referee/tests/test_math.rs` (std/math against Python on generated cases, in every backend, and what stops the program)
- `referee/tests/test_radix_literals.rs` (hexadecimal and binary literals in every spelling against Rust's arithmetic, in the VM, under aipl-run, and natively, in constants and enum values, both toolchains' bytes alike, and the near-literals both refuse in the same words)
- `referee/tests/test_must_use.rs` (a dropped result or option is refused, `(drop e)
- `referee/tests/test_assert.rs` (assert and abort stop with their message in every backend, the same bytes and errors from both toolchains, no_leak)` ignores one on purpose, in both toolchains and every backend)
- `referee/tests/test_bit_ops.rs` (`rotl`, `rotr`, `bitnot`, `clz`, `ctz`, and `popcnt` on generated operands and counts against Rust's `rotate_left`, `rotate_right`, `!`, `leading_zeros`, `trailing_zeros`, and `count_ones`, in the VM, under aipl-run, natively, and as an AArch64 executable where one can run; folded in constants; both toolchains' bytes and refusals alike)
- `referee/tests/test_math_float.rs` (std/math's exp, ln, pow_f64, sin, cos, atan2, and the other fdlibm ports against the `libm` crate bit for bit on 73,000 generated inputs, under aipl-run, natively, and in the VM, and within 1 ulp of the system's libm)
- `referee/tests/test_stats.rs` (std/stats against Python's statistics module to the bit on generated data, and its other functions, in every backend)
- `referee/tests/test_libs.rs` (std/set, std/random and std/uuid, std/path against Python's posixpath, and std/cli with its usage and exit statuses, in every backend)
- `referee/tests/test_encode.rs` (std/encode against Python's base64, binascii, and urllib.parse on generated bytes and near-miss texts, and its API, in every backend)
- `referee/tests/test_regex.rs` (std/regex against Python's re on 1,500 generated patterns and texts, and its API, including a pattern exponential for backtracking, in every backend)
- `referee/tests/test_memory.rs` (a view of a freed buffer, of a buffer that grew, and a reader's line after close each stop the program with "Use after free" in every backend; a memory reader's line outlives it; `(no_leak)`'s refusals with their chains, a `(no_leak)` handler on a request arena run 10,000 times with memory flat, and one reading and writing through four kinds of stream; `--max-memory` stopping a program at the same place in the VM, under aipl-run, natively, and in the bundle, both toolchains' module alike; os.arg and os.env reading their tables once; a `(no_leak)` handler over std/url, std/json, and std/text run 10,000 times with memory flat)
- `referee/tests/test_web.rs` (std/url against Python's urlsplit, parse_qsl, and urlencode and RFC 3986's resolution, its examples and its algorithm on generated references (Python's urljoin differs from the RFC on 32 of 300: it keeps dot segments in a network-path reference, drops empty segments, and returns the base whole for an empty reference); std/html against Python's html module; std/http's reading, refusals, limits, and writing; json.get_path; std/ip against Rust's IpAddr and SocketAddr parsers on 8,000 generated texts; in every backend)
- `referee/tests/test_exit.rs` (main's i32, void, and (result i32 str), and sys.exit, give the same status, output, and error output in the VM, under aipl-run, natively, and in the launcher bundle, a status outside 0-125 an error with 134 everywhere; both toolchains' bytes alike)
- `referee/tests/test_serve.rs` (std/serve answering real requests, refusals included, with the same bytes from the VM, aipl-run, a native executable, and the launcher bundle; 100,000 requests natively and under aipl-run (a million with AIPL_MILLION=1), and 20,000 in the VM, with the heap where it was after the first thousand; a view kept past its request stopping the program with "Use after free" in every backend; serve.once answering saved requests piped through it with the same bytes in the VM, under aipl-run, and natively; `aipl serve --fresh`, a native executable built with `--serve`, and the bundle built with it each answering 500 for a handler that traps and for an answer over 64 MiB, and serving the next request; an address in use refused with the same message by each, and a threaded program refused when compiled; the self-hosted `aipl serve --fresh` answering the same, stopping after `--requests` (0 being none), and leaving no temporary executable; both toolchains' bytes alike)
- `referee/tests/test_sys.rs` (`fs.chmod`: the same results and the same files' modes from the VM, aipl-run, a native executable, and the launcher bundle, with and without `--sandbox`, and from a native executable where `openat2` is refused; the process operations: a pipe each way through `cat`, exit statuses and a signal, output and error through one pipe, arguments and environment, every refusal, `exec` taking over the program with its status, and the grants of `--allow-run` (none, one, two, `*`), and `std/proc` over them (running a program, a conversation through pipes, the error on its own pipe, the PATH lookup, `exec`), `std/fs`'s `chmod`, `rename`, `remove`, `mkdir`, and `rmdir` (their errors as `io.Error`s, the modes and files they leave), and `fs.rename` (a file replaced, a directory and a link moved, the errors of each mismatch, what the working directory and `--sandbox` confine, and from a native executable where `openat2` is refused), and `fs.rmdir` (an empty directory removed, the errors for one that is full, a file, a link, or a missing path, the same confinement and fallback), and `fs.mkdir` (a directory made with the mode the umask leaves, the errors for a path that exists, a missing parent, or a file as parent, the same confinement and fallback), and `sys.poll` with `std/time`'s `sleep` (a 300 ms wait, none for zero or less, the error for no subscription), alike from the VM, aipl-run, a native executable, and the launcher bundle; both toolchains' bytes alike)
- `referee/tests/test_net.rs` (a server whose `(no_leak)` handler serves four connections over loopback, IPv4 and IPv6, with the same bytes from the VM, aipl-run, a native executable, and the launcher bundle, its arena flat; a listen without a grant, or with another one, refused everywhere; both toolchains' bytes alike)
- `referee/tests/test_deflate.rs` (std/deflate against Python's zlib and gzip both ways: Python's streams of every level and strategy, whole, with a bit flipped, and cut short, and ours checked by Python; and its API, in every backend)
- `referee/tests/test_hash.rs` (std/hash against Python's hashlib, hmac, and zlib on every length up to a few blocks, NIST's and RFC 4231's cases, and its API, in every backend)
- `referee/tests/test_io.rs` (a failed read or write gives an `io.Error` of the same kind in the VM, under aipl-run, and natively, from the one WASI errno table; both toolchains' bytes alike)
- `referee/tests/test_scopes.rs` (a name declared again in a sibling block, arm, loop, or clause, with another type, and in a template, in every backend and both toolchains' bytes; shadowing a name still visible, one message in both)
- `referee/tests/test_showcase.rs` (examples/jsonpp.aipl, the JSON pretty-printer: its own tests, forty generated documents laid out as Python's `json.dumps` lays them out, as wasm and as an executable, and every option and failure message)
- `referee/tests/test_portable.rs` (a copied `aipl` running every command from the library built into it; `AIPL_STD` for programs only; the self-hosted `aipl` from a `bin/` and `share/aipl/std/` layout giving the Rust command's bytes)
- `referee/tests/test_opt.rs` (a trap in an inlined function reported at its call under aipl-run and natively, `--no-opt` keeping its frame; the module cache surviving a damaged entry and turned off)
- `referee/tests/test_bootstrap.rs` (the checked-in seed builds the compiler, and its build is byte for byte the Rust toolchain's; a compiler replaces the file it is running from and leaves no temporary file, a failed write included; `aipl run` building, starting, and removing a program, with its arguments, input, and exit status; the AIPL `aipl` command against the Rust one, on every example, benchmark, and program below tests/aipl; and `aipl compile --exe --target arm64` writing the AArch64 executables the Rust command writes, the compiler's own among them), and the native backend's `referee/tests/test_native_arm64.rs` (the AArch64 encoder against Apple's assembler, in the VM and compiled to wasm; the ELF writer's AArch64 layout; the first AArch64 executable built alike in the VM and compiled to wasm, and run where AArch64 code can run)
- `referee/tests/test_native_arm64_programs.rs` (AIPL programs compiled to AArch64 executables and run where AArch64 code runs: exit status, integer arithmetic, loops and calls, printing, files, directories, listing, and file information, a trap's reason and call chain, an out-of-bounds access, the process operations: pipes, statuses, signals, and `exec`, `--sandbox`, sockets, `--serve`, threads, and the benchmarks' output)
- `referee/tests/test_native_reader.rs` (the wasm reader against wasmparser)
- `referee/tests/test_native_x64.rs` (the encoder against GNU as)
- `referee/tests/test_native_elf.rs` (runs the first native executable), and `referee/tests/test_native.rs` (the native backend's harness: every program built as wasm and natively, run under `aipl-run` and directly, with identical output, error output, exit status, and files; the AIPL compiler built natively and reproducing itself). `referee/tests/test_differential.rs` also checks that allocation grows memory to the same page count in both backends. `referee/tests/test_windows.rs` (Windows only: processes, exec and absolute-path arguments, run in the `windows` CI job) has no tests elsewhere.

### 10.3 Opcode conformance contract

`referee/tests/test_opcode_conformance.rs` holds a minimal program for every `OpCode` variant. The `match` over `OpCode` has no wildcard arm, so adding a variant to `referee/src/ast.rs` without a program is a compile error. For each variant the test asserts exactly one of:

- **(a)** the VM returns `Ok`, the wasm backend returns bytes, and `wasmparser` validates them; or
- **(b)** the checker, the VM, or the wasm backend returns `Err`.

An op that "succeeds" by returning a default value in one backend and a no-op in the other satisfies neither and fails the suite. New opcodes must be added to this file in the same commit that adds them to the AST.

### 10.4 Differential testing (`referee/tests/test_differential.rs`)

This is what makes "wasm semantics are the spec" enforceable. `wasmtime` is a dev-dependency; the harness compiles a module with `WasmCompiler`, instantiates the bytes in wasmtime (no imports are needed), and calls the export. `differential(module, wasm, fn_name, args)` runs the same call in a fresh `VM` and a fresh wasmtime instance and asserts one of:

- both return the same value (`bool` is normalised to `Int(0|1)`, its wasm shape), or
- both fail (VM `Err` and wasmtime trap, e.g. division by zero).

Contract and bounds failures must happen in both: the compiled message (read from cells 92 and 96) must be the VM's, without the position and the values compiled code cannot show for a contract. Any other mismatch panics with `DIVERGENCE ... (fix referee/src/vm.rs)`. The wasm side is never changed to match the interpreter.

Coverage:

- Every edge case in AIPL_SPEC.md 8 and 8.1 as a single-expression program.
- Control flow: `for` bounds (the end left out), negative-start loops, `while` with a stepped counter, `while` with `set!`, nested `if`/`block` values, recursion, memory round trips through the bump allocator.
- Typing and scoping (P7): one case per rule.
- Structs and arrays (P8): `i32`, `bool`, `i64`, `f32`, `f64` fields at their aligned offsets; `sizeof` with padding; `i32` and `i64` arrays including the length header and cursor advance; a size expression that itself allocates; the heap addresses left by `ok`/`err` cells and by an allocation inside a result payload; negative `arr.new` sizes and `put`/`arr.set` into the reserved block failing in both; out-of-range indexes failing in both with the same message; and the AIPL_SPEC.md 4.E example.
- Value structs held inline (docs/design/VALUES_PLAN.md): their sizes and offsets, raw words written through a field path, that results and unions holding one take no heap, array element spacing, and reads through a null pointer or past an array's end and writes into the reserved block failing in both.
- Every `examples/*.aipl`: resolved, checked, compiled; every function returning `i32`/`i64`/`bool` with all-`i32` params is called over nine fixed argument tuples in both backends. Files the wasm backend rejects are skipped with a printed reason, and files that do I/O (`word_count`, `word_freq`) are compared with captured output in `referee/tests/test_wasi.rs` instead. Every example must parse (the stale-file list is empty). The test asserts at least 4 files and 40 calls were compared so it cannot silently go vacuous; today it compares `accounts`, `math_core`, `matrix_mult`, and `quicksort` over 116 calls. Every example is also compiled by the self-hosted compiler at byte parity (`referee/tests/test_selfhost.rs`).
- `aipl_src/codegen.aipl`'s `test_signatures_and_locals` is compared under WASI (the module uses `fs.*` and so imports WASI).

Sample argument values are deliberately small. Example functions use parameters as loop bounds, and a tree-walking VM asked to iterate `i32::MAX` times is not a test, it is a hang. Wrap-around is covered by the explicit expression cases instead.

### 10.5 I/O under WASI (`referee/tests/test_wasi.rs`)

Compiled modules that print or touch files import from `wasi_snapshot_preview1`, so they need a WASI host. The tests build one with `wasmtime-wasi`: a `WasiCtxBuilder` with stdout captured into a `MemoryOutputPipe`, stderr inherited, and a fresh temporary directory preopened as `.` with read-write permission, linked through `wasmtime_wasi::p1::add_to_linker_sync`. The VM side of each comparison runs with the process cwd switched to its own temporary directory (under a lock, since cwd is process-global) so both backends see an empty directory.

Covered: `aipl_src/file_io.aipl`'s `run_file_io_tests` returns 1 in both backends and leaves no file behind; `sys.print` output is exactly one line per argument; interned string literals report their length and compare by identity; 600 bytes of literals compile and run in both backends, and literals past the 1 MiB limit fail to compile with a message; opening a missing file returns -1 in both; a write-then-read round trip agrees byte for byte and the wasm side's file is visible on the host in the preopened directory; `sys.exit 7` surfaces as `I32Exit(7)` from wasmtime and as `sys.exit(7) requested` from the VM; and a module with no I/O has no import section and still instantiates with no imports.

To run compiled I/O outside the tests: `aipl run module.wasm` (the working directory is preopened), or build an executable with `aipl compile --exe`.

### 10.6 Self-hosted byte parity (`referee/tests/test_selfhost.rs`)

`self_host(src)` runs `codegen.compile_module` (codegen.aipl compiled to wasm by the Rust toolchain, under wasmtime, a fresh instance per program; in the VM until 2026-10-20, a hundred times slower), validates the output with `wasmparser`, and returns the bytes or the compile error code. `assert_self_hosted_matches_rust` asserts the whole module equals `WasmCompiler::compile` for the same source and prints the first differing function and byte if not. Coverage: minimal, `add`, `i64` (literals at both extremes, unsigned ops, an `i64` struct field after a `bool`, `i64` arrays, `mem.load64`/`store64`), `f32`/`f64` parameters, arithmetic and struct fields, 524 float literals (200 random short ones, 300 random ones with up to 40 digits and exponents to ±330, and hard cases: halfway points, subnormals, infinities), `(ok:T v)` with compound `T`, an escaped quote inside a string, `compute` (loop + call), structs with `bool`/`str` fields, arrays, `sys.print`, results (including `match_result` as a statement), file I/O, repeated string literals with a `str` let plus `mem.alloc`/`mem.grow`/`sys.exit`, a mixed program, void `if` with block-scoped `let`s and an empty `(block)` else, a store, arrays plus results, `aipl_src/memory.aipl`, `aipl_src/compiler.aipl`, and codegen.aipl compiling itself (under wasmtime and, the longest of these tests, in the VM too: the one test that keeps the VM running the whole compiler). Behavioural checks run the self-hosted output in wasmtime: the store guard traps on bytes 0-3 and 64-1023 and nowhere else in a module without literals, and also on the literals in a module with 800 bytes of them, whose heap then starts at 1832 in both the VM and wasm; arrays and results compute the same values as the VM and a negative `arr.new` traps; a file write plus `sys.print` under WASI produces the file and the exact stdout. Compile errors 95, 96, and 768 are asserted for a result-typed field, unknown structs, and a 70 000-byte literal (over the self-hosted 64 KiB buffer).

**Bootstrap fixpoint.** `self_hosted_compiler_reproduces_itself_under_wasmtime` compiles the self-hosted compiler with the Rust backend (stage 1), runs that wasm module under wasmtime on its own source, and requires the output (stage 2) to be byte-identical to stage 1. The second compile involves neither the VM nor any Rust compiler code, and takes about 20 ms. Since P9, codegen.aipl itself uses `i64` and `f64` arithmetic (literal parsing, LEB128, float literal bits), so the fixpoint also covers those paths of the self-hosted compiler.

### 10.7 Documentation examples (`referee/tests/test_doc_examples.rs`)

Every ```` ```lisp ```` block in `PROMPT_GUIDE_FOR_AIS.md`, `README.md`, and this specification and docs/IMPLEMENTATION.md is checked. A complete module must parse, check, and compile, and if it defines `main`, `main` must return the value listed in the test's `EXPECTED` table in both the VM and wasmtime (with WASI and a scratch directory); the few spec modules that cannot run that way are listed with a reason in `SPEC_SPECIAL`. A fragment of this specification (a few definitions without `(module`) is wrapped in a module and type-checked; the deliberately invalid example in 7.3 must fail with its documented message. Adding an example with a `main` means adding its expected value there. This test found that a `match_result` used as a statement compiled to invalid wasm.

---

## 11. Reference tables

### 11.1 Runtime cells (bytes 0..1024)

What each cell below the string literals holds, and who owns it. AIPL_SPEC.md 7.9 says which of these a program may rely on (the heap cursor, the string literals and the heap start) and that the rest are not for programs.

| Address | Width | Owner | Meaning |
|---|---|---|---|
| 0 | i32 | allocator | heap cursor: the next address `mem.alloc` will return. Initialised to `1024` by `VM::new` and by the wasm data segment. |
| 4 | i32 | codegen | the first compile error's code (`0` = none; section 5), read by the host |
| 8..12 | | unused | |
| 16 | i32 | codegen | pointer to the keyword map, built once by `codegen_init` and reused by later compiles |
| 20 | | unused | |
| 24 | i32 | std/testing | the failures in the test running (AIPL_SPEC.md 4.L) |
| 28 | i32 | std/os | pointer to the command line's table, read by the first `os.arg` or `os.arg_count` (0 until then) |
| 32 | i32 | std/os | pointer to the environment's table, read by the first `os.env` (0 until then) |
| 36..40 | | unused | (the rest of a compile's state is in the `Cg` struct `compile_module` creates) |
| 44, 48 | i32, i32 | codegen | source offset and length of the callee name, after compile error 1452, read by the host |
| 52 | i32 | std/alloc | pointer to the shared default allocator, made by the first `alloc.default` (0 until then) |
| 56 | i32 | std/gen | the counter fresh generations come from |
| 60 | i32 | codegen | pointer to the module `compile_module` emitted, read by the host |
| 64, 68 | i32, i32 | WASI runtime | iovec 0: buffer pointer, length (used by `sys.print`, `fs.read`, `fs.write`) |
| 72, 76 | i32, i32 | WASI runtime | iovec 1: the interned `"\n"` and length 1 (`sys.print`) |
| 80 | i32 | WASI runtime | `nwritten` / `nread` out-parameter |
| 84 | i32 | WASI runtime | `path_open`'s opened-fd out-parameter |
| 88 | i32 | threaded modules | 1 once the shared memory has been initialised (section 3) |
| 92, 96 | i32, i32 | compiled checks | address and length of a failed check's message (`Array index out of bounds: ...`, or a contract's `Pre-condition failed in ...`, written at the heap cursor), set just before the `unreachable` that ends the program; `aipl-run` and native executables print it instead of the trap (0 = none) |
| 128..256 | | compiled checks | where a failed bounds check writes its message |
| 88..1024 | | reserved | not handed out by `mem.alloc`; do not use |
| 1024..heap start | | string literals | interned string literals, `[len u32 LE][bytes]` each, in first-use order; a `str` value points at the bytes. Empty for a module without literals. Read-only |
| heap start.. | | heap | `mem.alloc` region. The heap start is the first 8-aligned address after the literals (1024 without literals); it is the initial value of the cursor at address 0 |

### 11.2 Host operations, backend by backend

Each host operation in the VM (the Rust API it calls), in the WebAssembly module (the WASI or `aipl_*_v1` import), and in a native executable; AIPL_SPEC.md 4.C has the meaning and the result.

| Op | VM | wasm lowering | Result |
|---|---|---|---|
| `(sys.print s ...)` | prints each argument on its own line | `fd_write` to fd 1 with a two-entry iovec (bytes, then `"\n"`); `str` arguments only | `void` |
| `(fs.open ptr len flags)` | `std::fs` open; `0` = read-only, else create+truncate for writing | `path_open` on the preopened directory (fd 3): `oflags = CREAT|TRUNC` and rights `FD_READ|FD_WRITE` when `flags != 0`, else rights `FD_READ` | new fd, or minus its WASI errno |
| `(fs.read fd buf max)` | | `fd_read` with one iovec | bytes read, or minus its WASI errno |
| `(fs.write fd buf len)` | | `fd_write` with one iovec | bytes written, or minus its WASI errno |
| `(fs.close fd)` | | `fd_close` | `0`, or minus its WASI errno |
| `(fs.delete ptr len)` | | `path_unlink_file` on fd 3 | `0`, or minus its WASI errno |
| `(fs.rename old_ptr old_len new_ptr new_len)` | `std::fs::rename` on the two paths as they are | `path_rename`, each path's directory (fd 3, or 4 for a path from `/`) chosen as `fs.delete` chooses it; natively each path's parent is opened beneath its directory (as `path_unlink_file` does) and `renameat` moves the last component between them. Replaces a file already at the new path (a directory only an empty one, `-55` otherwise; a directory onto a file `-54`, a file onto a directory `-31`); a link is moved itself | `0`, or minus its WASI errno. A relative path stays in the working directory (`-63` for a `..` or link leading out, on either side); under `--sandbox` there is no `/` (`-8`) |
| `(fs.rmdir ptr len)` | `std::fs::remove_dir` on the path as it is | `path_remove_directory` on fd 3 (or 4 for a path from `/`), chosen as `fs.delete` chooses it; natively the parent is opened beneath its directory (as `path_unlink_file` does) and `unlinkat` with `AT_REMOVEDIR` removes the last component | `0`, or minus its WASI errno: a missing path `-44`, a directory with anything in it `-55`, a file or a link (even to a directory) `-54`. A relative path stays in the working directory (`-63` for a `..` or link leading out); under `--sandbox` there is no `/` (`-8`) |
| `(fs.mkdir ptr len)` | `std::fs::create_dir` on the path as it is | `path_create_directory` on fd 3 (or 4 for a path from `/`), chosen as `fs.delete` chooses it; natively the parent is opened beneath its directory (as `path_unlink_file` does) and `mkdirat` makes the last component, mode 0777 less the umask | `0`, or minus its WASI errno: a path that exists (a file or a link too) `-20`, a missing parent `-44`, a file as the parent `-54`. A relative path stays in the working directory (`-63` for a `..` or link leading out); under `--sandbox` there is no `/` (`-8`) |
| `(fs.opendir ptr len)` | `std::fs::read_dir`; the names held from the start, `.` and `..` first, as wasmtime gives them | `path_open` on fd 3 with `oflags = DIRECTORY` and rights `FD_READDIR` | new fd (for `fs.readdir` and `fs.close`), or minus its WASI errno |
| `(fs.readdir fd buf len cookie:i64)` | | `fd_readdir`: WASI dirents (`d_next` u64, `d_ino` u64, `d_namlen` u32, `d_type` u8, 3 bytes, then the name) from the entry `cookie` names (`0` the first, else a `d_next`); the last one may be cut off | bytes written (`len` when more may follow), or minus its WASI errno |
| `(fs.stat ptr len out)` | `std::fs::metadata` | `path_filestat_get` on fd 3, following symbolic links: a 64-byte WASI filestat at `out` (the filetype u8 at 16: 3 a directory, 4 a regular file; the size at 32; access, modification, and change times at 40, 48, 56 in nanoseconds) | `0`, or minus its WASI errno |
| `(fs.chmod ptr len mode)` | `std::fs::set_permissions`, the path confined as below | `chmod(dirfd, ptr, len, mode)` from AIPL's own import module `aipl_sys_v1` (docs/design/SYS_PLAN.md), provided by aipl-run, the directory (fd 3 or 4) chosen as `fs.delete` chooses it; natively the file is opened beneath that directory as `fs.open` opens one, read-only, then `fchmod` and `close` (a file that cannot be read: `fchmodat` on its parent, unless it is a symbolic link) | `0`, or minus a WASI errno. `mode` is the permission bits, 0 to 0o777; a bit outside them (setuid, setgid, sticky) is `-28`. A relative path stays in the working directory (a `..` or a symbolic link leading out is `-63`); under `--sandbox` there is no `/` (`-8`). The VM's paths are ordinary: nothing confines them. A path whose length runs past memory is `-21`, a value and not a trap |
| `(proc.spawn path len args args_len stdio)` | `std::process::Command` on the path exactly as given (no `PATH` search), if `--allow-run` names it (`*` for any); the arguments are `args_len` bytes at `args`, each followed by a zero byte (a last word without one counts), the first the program's own name; `stdio` is three 2-bit fields, standard input in bits 0-1, output in 2-3, error in 4-5: 0 the program's own, 1 a pipe, 2 the null device, and 3 for the error only, the output's | `spawn` from `aipl_sys_v1` (docs/design/SYS_PLAN.md D3), provided by aipl-run; natively `pipe2`, `fork`, `dup2`, `execve` (a pipe that closes on exec tells the parent why it failed) | a process handle, or minus an errno: `-76` (not granted), `-44` and the other errnos of a program that cannot start, `-28` for a `stdio` outside those values, `-21` for a path or arguments outside memory, `-1` for more than 65,536 bytes of both. The environment and working directory are the program's. Handles are not descriptors, and what a number is differs between backends: print none |
| `(proc.stream p which)` | the pipes `spawn` made | `aipl_sys_v1.stream` | the pipe of process `p`'s input (0), output (1), or error (2); `-1` if that stream is not a pipe, `-8` if `p` is not a process this program started |
| `(proc.read h buf max)`, `(proc.write h buf len)` | `Read` and `Write` on the pipe | `aipl_sys_v1.read`, `.write`; natively `read`, `write` (SIGPIPE ignored for the call) | bytes moved: a read waits for some and gives `0` at the end; `-8` for a handle that is not a pipe this program has open, or one of the wrong way round (the input is written, the output and error read), `-21` for a buffer outside memory, `-64` for a write to a program that has gone |
| `(proc.close h)` | drops the pipe, or the process (which goes on running) | `aipl_sys_v1.close`; natively `close`, or the slot freed | `0`, or `-8`. Closing the input is how a program is told its input has ended |
| `(proc.wait p)` | `Child::wait` | `aipl_sys_v1.wait`; natively `wait4` | the program's exit status 0 to 255, or 256 plus the number of the signal that ended it (`kill -9` is 265); the same again on a second wait; `-8` for a process that is not this program's |
| `(proc.try_wait p)` | `Child::try_wait` | `aipl_sys_v1.try_wait`; natively `wait4` with `WNOHANG` | `proc.wait` that does not wait: the status if the program has ended (and it is waited for, so the same again), `-6` (EAGAIN) while it runs, `-8` for a process that is not this program's |
| `(proc.kill p sig)` | `kill(2)` on the program's id, which a waiting thread's lock does not hold up | `aipl_sys_v1.kill`; natively `kill` | `0`; `-8` for a process that is not this program's or has been waited for (its id may be another process's now), `-28` for a signal outside 1 to 64. One thread may kill what another is waiting for |
| `(proc.poll h events timeout_ms)` | `poll(2)` on the pipe | `aipl_sys_v1.poll`; natively `poll` (`ppoll` on AArch64) | waits at most `timeout_ms` (-1: no limit, 0: only look) until pipe `h` can be read (`events` 1: the read end of a program's output or error: data, its end, or an error) or written (2: the write end of its input, room for a pipe's worth): `events` if so, `0` if the time ran out, `-8` for a handle that is not such a pipe, `-28` for other events |
| `(proc.exec path len args args_len)` | `CommandExt::exec`, if granted | `aipl_sys_v1.exec`; natively `execve` | does not return when it works (the program ends with the new program's status); otherwise minus an errno, as `proc.spawn` gives |
| `(net.listen addr len port)` | `std::net::TcpListener::bind`, if `--listen` granted the address and port | `listen` from AIPL's own import module `aipl_net_v1` (docs/design/NET_PLAN.md section 3), provided by aipl-run; natively `socket`, `setsockopt(SO_REUSEADDR)`, `bind`, `listen(128)` | a socket's handle; `-1` (a malformed address, a port outside 0-65535, or the system's refusal), or `-2` if not granted. The address is its `len` bytes at `addr`: 4 for IPv4, 16 for IPv6 |
| `(net.accept h)` | `TcpListener::accept` | `aipl_net_v1.accept`; natively `accept4` | the connection's handle, or `-1` |
| `(net.connect addr len port)` | `TcpStream::connect`, if `--allow-connect` granted the address and port (`ADDR:*` grants any port) | `connect` from `aipl_net_v1`, provided by aipl-run; natively `socket`, `connect` | the connection's handle (for `net.recv`, `net.send`, `net.close`); `-1` (a malformed address, a port outside 0-65535, or a refused or failed connection), or `-2` if not granted. The address is its `len` bytes at `addr`: 4 for IPv4, 16 for IPv6; there is no name resolution |
| `(net.connect_host name len port)` | `TcpStream::connect` to the host whose name (or address as text) is the `len` bytes at `name`, found by the system's resolver, if `--allow-connect` granted that name (ignoring case) or address and the port | `connect_host` from `aipl_net_v1`, provided by aipl-run; natively `-3` | the connection's handle; `-1` (a malformed name, a bad port, or a refused or failed connection), `-2` if not granted, `-3` where there is no resolver (a native executable), `-5` if the name does not resolve. A name grant lets the program reach whatever the name resolves to |
| `(net.tls_connect h name len)` | rustls: makes connection `h` secure, as a client of the server whose certificate must be trusted (the Mozilla roots, and the certificates of the PEM file `AIPL_CA_FILE` names) and carry the name at `name` (a host name, or an address as text) | `tls_connect` from `aipl_net_v1`, provided by aipl-run; natively `-3`, on which std/net starts `aipl-net` (below) | `0` (the handle now carries plain bytes over TLS: `net.recv`, `net.send`, `net.close` as before); `-1` if the handshake fails (an untrusted or mismatched certificate, a server that does not speak TLS), `-3` where there is no TLS (a native executable), `-6` for a bad name. A server's close without TLS's close_notify reads as the end |
| `(net.recv h buf max)` | `TcpStream::read` | `aipl_net_v1.recv`; natively `read` | bytes read into `buf`, `0` at the end of the connection, or `-1` (also for a buffer outside memory: not a trap) |
| `(net.send h buf len)` | `TcpStream::write` | `aipl_net_v1.send`; natively `sendto` with `MSG_NOSIGNAL` (a peer gone is `-1`, not a signal) | bytes written (perhaps fewer than `len`), or `-1` |
| `(net.poll h events timeout_ms)` | `poll(2)` on the socket (a secure connection's already decrypted bytes count) | `aipl_net_v1.poll`; natively `poll` (`ppoll` on AArch64) | waits at most `timeout_ms` (-1: no limit, 0: only look) until socket `h` can be read (`events` 1: data, the end of the data, an error, or, for a listening socket, a connection to accept) or written (2): `events` if so, `0` if the time ran out, `-1` for a handle that is not an open socket, other events, or an error |
| `(net.close h)` | shuts a connection down, then closes it | `aipl_net_v1.close`; natively `shutdown`, `close` | `0`, or `-1` for a handle that is not an open socket. Socket handles are not file descriptors: `fs.close` does not close one, and the numbers mean different things in different backends (std/net keeps them private) |
| `(sys.exit code)` | returns the error `sys.exit(N) requested` rather than killing the host process | `proc_exit` | never returns |
| `(sys.time)` | the system clock | `clock_time_get` (realtime) | `i64` nanoseconds since 1970-01-01 UTC |
| `(sys.monotonic)` | a monotonic clock | `clock_time_get` (monotonic) | `i64` nanoseconds from an arbitrary fixed start; never decreases. Use for durations |
| `(sys.random ptr len)` | `/dev/urandom` | `random_get` | `0`, or minus its WASI errno; fills `len` bytes at `ptr` from the OS's secure generator |
| `(sys.poll subs out n nevents)` | waits as the one relative clock subscription at `subs` says (`std::thread::sleep`); the event goes to `out`, the count 1 to `nevents` | `poll_oneoff`, as it is (any subscriptions WASI hosts give); natively a `nanosleep` of the timeout, for one subscription only | `0`, or minus its WASI errno. The VM and native executables take one relative clock subscription (tag 0, flags 0): `n` not 1 is `-28`, another kind `-58`; a wait a signal cuts short ends early, as success. `std/time.sleep` builds the subscription |
| `(args.sizes count_ptr size_ptr)` | the host's argument list (`aipl eval FILE -- a b` gives `FILE a b`; empty unless set) | `args_sizes_get` | `0`, or minus its WASI errno; writes the argument count and the total bytes of the NUL-terminated arguments |
| `(args.get argv_ptr buf_ptr)` | | `args_get` | `0`, or minus its WASI errno; writes one u32 address per argument at `argv_ptr` and the NUL-terminated arguments at `buf_ptr` |
| `(env.sizes count_ptr size_ptr)` | the process environment (a host may replace it) | `environ_sizes_get` | as `args.sizes`, for `KEY=VALUE` entries |
| `(env.get env_ptr buf_ptr)` | | `environ_get` | as `args.get` |

### 11.3 Recursion depth, backend by backend

AIPL_SPEC.md 7.11 guarantees 10,000 nested calls of a small function. What each backend reaches:

Calls nest as deep as the stack allows. Running out of stack stops the program with a message in every backend, never a crash:

| Backend | Room | Nested calls of a small function | Failure |
|---|---|---|---|
| `aipl-run` (wasm) | wasmtime's 512 KiB wasm stack | about 32,000 | `prog.wasm: wasm trap: call stack exhausted`, the call chain, exit 134 |
| native executable | 8 MiB per thread, the main thread's included (its own stack, so `ulimit -s` does not matter), below a guard page | about 130,000 (x86-64), about 70,000 (AArch64, whose frames are larger) | the same, with the program's name |
| VM | 192 MiB of the thread's stack | about 40,000 | `Call stack exhausted calling 'f': calls are nested too deeply (a recursion that does not end?)` |

A call's room is its frame: a function with many locals or deeply nested expressions gets fewer calls. Code meant to run under `aipl-run` should stay well below 30,000 nested calls; walk long lists and deep structures with `while`. In a native executable, each function's prologue compares the stack pointer, less the frame it is about to push, with a limit 256 KiB above the stack's low end and traps below it, which leaves room for the host routines and the trap's own output.

### 11.4 Value structs, backend by backend

Every backend runs value structs with the same results. The VM keeps a value as its fields' values. The wasm backend flattens one into its scalars: a `Particle` local is five wasm locals, a parameter five parameters, a result five results (multi-value).

### 11.5 `aipl test`

`aipl test FILE` runs each test of the file in a fresh process, so tests share no state: under aipl-run by default, natively with `--native`, in the VM with `--vm`, or in all three with `--all-backends`, where a test passes only if every backend passes it with the same output. `aipl test DIR` runs every file below DIR that has tests, and `-k TEXT` only the tests whose names contain TEXT. It prints the file, then `ok` or `FAIL` and each test's name, a failing test's output indented under it, and a total, `7 tests: 3 passed, 4 failed`; the exit status is 1 if any failed. A file with no tests runs its `--func` entry point as before (default `run_all`).
