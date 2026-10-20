# AIPL Revisions: Production Readiness & Constraint-Driven AI Design

**Date:** 2026-10-16  
**Context:** Post-`GENERATIONS_PLAN.md` (generational memory safety, `std/par`) and alongside `STDLIB_PLAN.md` (batches L1–L7: string interpolation, text/UTF-8, CSV/stats/set/random/CLI/path, encodings/hashes, Pike-VM regex, deflate/gzip, URL/HTTP/HTML, and L7 TCP sockets).

This document lays out the next architectural phases for AIPL across two fronts:
1. **Constraint-Driven AI Differentiators:** Compiler and language mechanisms that *mechanically constrain the solution space* so AI coding agents produce correct code faster, cannot silently take shortcuts, and receive machine-actionable feedback when they make a mistake.
2. **Production Language Completeness:** The core language ergonomics, memory-model fixes for long-running processes, OS/runtime primitives, and compiler optimizations needed to take AIPL from a self-hosted systems prototype to a straight-up production language.

---

## Part 1: Killer Differentiators — Constraining the Space for AI Agents

Prompt guides and examples only go so far. Models still hallucinate argument orders, forget cleanup on early returns, silently weaken tests when their code fails, or lose track of invariants in 500-line files.

Because AIPL is a closed, S-expression language with explicit types, no ambient authority, and built-in contracts (`req`, `ens`, `inv`), the compiler can do things that Rust, Go, Zig, and Python compilers cannot. Every feature below is designed around one principle: **turn an implicit convention that an AI might forget into a mechanical constraint the compiler enforces or solves.**

### 1. Spec-Locked Verification (`aipl verify --against <spec.aipl>`)

**The AI failure mode:** When an agent is asked to implement a module or fix a failing test, it can "cheat" by changing a function signature, weakening a `(req ...)` or `(ens ...)` contract, dropping a required `(pub)` function, or sprinkling `(unsafe ...)` into the code to bypass checks.

**The constraint:**
* Allow `.spec.aipl` files (or `(spec ...)` declarations) containing only type definitions (`struct`, `enum`, `union`) and function headers (`fn` signatures, `(pub)`, effect tags, and `req`/`ens`/`inv` contracts, with no bodies—identical to `interface` signatures).
* `aipl verify --against math.spec.aipl math.aipl` checks `math.aipl` and mechanically verifies that:
  1. Every public type, field visibility, and function signature in `math.spec.aipl` exists in `math.aipl` with the exact same types.
  2. Every `req`, `ens`, and `inv` contract in the spec is present in the implementation **AST-for-AST** (the agent may add stricter internal helper contracts, but cannot alter or remove the spec's contracts).
  3. No `(unsafe)` blocks or clauses appear in `math.aipl` unless explicitly permitted by the spec.

**Why it changes how agents work:** A planner agent (or human architect) writes a 30-line `.spec.aipl` defining the types and contracts of a task. The coding agent is locked in a box: it cannot touch the spec, cannot weaken the contracts, and cannot use `unsafe`. It only fills in the bodies until the compiler and contract fuzzer say `OK`.

### 2. Zero-Config Contract Fuzzing & Counterexample Shrinking (`aipl verify --fuzz`)

**The AI failure mode:** Code compiles and passes the 2 happy-path tests the agent wrote, but crashes or returns the wrong answer on `n = 0`, an empty array `[]`, negative numbers, `i32::MIN`, or duplicate elements.

**The constraint:**
* `aipl verify` currently says *"Contracts are type-checked, not proven."* While full SMT theorem proving is brittle, **automated structure-aware fuzzing from contracts** is fast, deterministic, and catches 95% of real bugs.
* Add `aipl verify --fuzz` (running in milliseconds via the compiled Wasm or native backend):
  1. For every function in the module, inspect its parameter types and `(req ...)` AST.
  2. Harvest literal constants and boundary values directly from the function's AST (e.g., if the function contains `(lt i 10)` or `(eq b 46)`, seed `9, 10, 11` and `45, 46, 47` alongside standard boundary values `0, 1, -1, i32::MIN, i32::MAX`, empty arrays, 1-element arrays, etc.).
  3. Generate input tuples, discard any that fail `(req ...)`, and execute the function.
  4. Fail immediately if any input triggers a trap (bounds check, null dereference, division by zero, `checked.*` overflow, use-after-free) or violates `(ens ...)` / `(inv ...)`.
  5. Shrink the failing input to the smallest counterexample and print the exact failing S-expression call:
     ```
     Contract failure in 'binary_search' (search.aipl:4:3):
       counterexample: (call binary_search (arr i32 [10]) 5)
       trapped at search.aipl:9:17: Array index out of bounds: index -1 for array of length 1
     ```

**Why it changes how agents work:** Every `(req ...)` and `(ens ...)` becomes an executable property test suite with zero test-harness boilerplate. The agent gets an exact failing input before it even finishes its turn.

### 3. Effect & Mutability Constraints (`(pure)`, `(no_bump)`, `(readonly)`)

**The AI failure mode:** You ask an agent for a pure calculation or a long-running request handler, and it mutates an input array in place (breaking the caller), reads a file or clock mid-calculation, or allocates on the immortal bump heap (`new`, `arr.new`, `mem.alloc`) inside a server loop.

**The constraint:**
Because AIPL has no ambient authority—every I/O operation, clock read, allocation, and mutation is an explicit primitive—the checker can enforce effect and mutability contracts with zero runtime cost:
* **`(pure)` clause on `fn` or `module`:** Rejects all host I/O (`fs.*`, `sys.print`, `sys.exit`, `args.*`, `env.*`), non-determinism (`sys.time`, `sys.monotonic`, `sys.random`), `unsafe`, and calls to non-`(pure)` functions. A `(pure)` function may allocate and mutate its own local variables/buffers, but cannot mutate any `(ptr S)` or `(arr T)` passed into it as a parameter unless marked mutable.
* **`(readonly x:(ptr S))` / `(readonly xs:(arr T))` parameters (or readonly by default, `(mut xs:(arr T))` to write):** Rejects `(put x ...)`, `(arr.set T xs ...)`, and passing `x`/`xs` to any non-readonly parameter.
  * *Bonus:* `wasm.rs` and `codegen.aipl` can feed `(pure)` and `(no_free)` directly into the loop-versioning pass (`GENERATIONS_PLAN.md`) to hoist generation and null checks across function calls!
* **`(no_bump)` clause on `fn` or `module`:** Rejects any use of the immortal bump allocator (`new`, `arr.new`, `mem.alloc`, `str.concat`, `buf.to_str`, `(format ...)`) in the function and its transitive callees, forcing all allocations to go through an explicit `alloc.Allocator` or `arena.Arena`. Putting `(no_bump)` on a server's request loop **proves at compile time** that a request cannot leak immortal memory.
* **Capability-restricted imports:** `(import third_party_json (allow pure))` or `(import plugin (deny fs env unsafe))` lets a production app import untrusted AI-written modules with a compile-time guarantee that they cannot touch the disk, environment, or raw memory.

### 4. Inline Executable Examples (`(example ...)`) Inside Function Headers

**The AI failure mode:** Agents write a function whose mental model of the math/indexing is slightly off, and only discover it much later when integrating the whole program.

**The constraint:**
Allow `(example (call fn_name args...) expected_expr)` right after `(ens ...)` inside any `(pure)` function definition:
```lisp
(fn gcd [a:i32 b:i32] -> i32 (pub) (pure)
  (req (and (gt a 0) (gt b 0)))
  (ens (gt res 0))
  (example (call gcd 1071 462) 21)
  (example (call gcd 7 13) 1)
  ...)
```
* **Checked at `aipl verify` time:** Whenever `aipl verify` or `aipl compile` runs, it evaluates every `(example ...)` on `(pure)` functions immediately. If `(call gcd 1071 462)` does not equal `21`, **verification fails with a compile error**.
* Because examples live inside the function header, they travel with the function in API summaries (`aipl context`) and can never go stale.

### 5. Typed Holes (`?`) & Synthesis-Guided Diagnostics

**The AI failure mode:** When an agent isn't 100% sure of a generic function's exact type arguments, field name, or parameter order, it guesses—producing cascading parse/type errors.

**The constraint:**
1. **First-class Typed Holes (`?`):** Allow `?` in any expression position (`(let x:i32 ?)` or `(call (map.get_or (ptr str.Bytes) i32) m ? 0)`) and in type-argument position (`(call (vec.push ?) v x)`).
2. **Actionable synthesis in the checker:** When the checker hits `?` (or a type mismatch!), it already has the local symbol table, struct definitions, and module signatures in memory. It reports:
   * The exact expected type `T` at the hole.
   * Every local variable, parameter, or `get` path on a local variable in scope that produces type `T`.
   * If `?` is in a generic type-argument position like `(call (vec.push ?) v x)`, the checker infers `?` from `v:(ptr (vec.Vec i32))` and prints:
     `file.aipl:12:19: Hole '?' in (vec.push ?) must be i32: replace with (call (vec.push i32) v x)`.
3. **Keep explicit source, infer in diagnostics:** AIPL's rule that source files have zero type inference is great for readability and unambiguous diffs. By doing **type inference inside the error message generator**, you keep the source language 100% explicit while giving the AI agent the exact copy-paste fix whenever it omits or miswrites a type argument (`vec.push v x` $\to$ `Hint: write (call (vec.push i32) v x)`).

### 6. Zero-Slack Compiler Hygiene (Unused = Error, Must-Use = Error)

**The AI failure mode:** An agent declares `(let Carrying:bool false)` or accepts a parameter `limit:i32`, writes the loop, forgets to actually check `limit` inside the loop, and the code compiles cleanly. Or it calls a function that returns `(result i32 str)` in statement position and forgets to check if it failed.

**The constraint:**
Make every form of "slack" in a function a hard checker error (in both `src/checker.rs` and `aipl_src/checker.aipl`):
* **Unused local variables or parameters:** Error (`parameter 'limit' is never used; prefix with '_' (_limit) if intentional`). In AI-generated code, an unused parameter or local is almost always a logic bug where the model forgot to wire a variable into an expression.
* **Unused `(result T E)` or `(option.Option T)` in statement position:** Error (`unused (result i32 str) from 'write_header': handle it with (try ...) or (match_result ...), or discard explicitly with (drop ...)`).
* **Unreachable expressions:** Any expression after `(return ...)`, `(break)`, or `(continue)` in the same block is a checker error.
* **Dead private items:** Any private `fn`, `struct`, `enum`, `union`, or `const` not reachable from `main`, `_start`, or a `(pub)` item is flagged.

### 7. Token-Minimal Context & Fault-Localization CLI (`aipl context`, `aipl trace`)

**The AI failure mode:** To use an existing multi-file codebase, an agent wastes thousands of context tokens reading implementation bodies, or wastes 5 turns inserting debug `println` calls to figure out why a function returned `-1`.

**The constraint:**
* **`aipl context <file.aipl | dir>`:** Parses the target file's imports (or a directory) and outputs *only* the canonical `(pub)` declarations, struct/enum/union layouts, effect tags, `(req ...)`/`(ens ...)` contracts, and `(example ...)` lines—stripped of all function bodies and private helpers. A 5,000-line library compresses into a 120-line exact specification that fits into any agent prompt with zero ambiguity.
* **`aipl trace <file.aipl> [--func NAME]`:** Runs the program (or failing test) with a lightweight ring buffer recording the last $N$ function entries, parameter values, and return values. On a contract failure, trap, or non-zero test return, it prints the call-tree value trace:
  ```
  Trace before failure (last 6 calls):
    parse_uint("12x")
      -> str.byte_at(b, 0) = 49 ('1')
      -> digit(49) = (ok 1)
      -> str.byte_at(b, 2) = 120 ('x')
      -> digit(120) = (err 120)
    <- parse_uint = -1
  ```
  This eliminates the "edit $\to$ add print statements $\to$ compile $\to$ run $\to$ remove print statements" loop for AI agents.

---

## Part 2: Core Language Revisions for Production

### 1. Early Error Propagation: `(try expr)` and `(try_opt expr)`
Deeply nested `(match_result ...)` blocks make multi-step I/O or parsing functions hard to generate without misplacing a closing parenthesis.
* **`(try expr)`**: Evaluates `expr` of type `(result T E)`. If `ok`, yields the `T` value; if `err`, returns `(err:FnOk e)` from the current function (whose return type must be `(result FnOk E)`).
* **`(try_opt expr)`**: Evaluates `expr` of type `(option.Option T)`. If `some`, yields `T`; if `none`, returns `(make (option.Option FnSome) none)`.
* **`(try_or expr fallback_err)`**: Converts an `(option.Option T)` into a `(result T E)` early return with `fallback_err`.
* **Implementation:** Pure AST desugaring before codegen (just like `format` in `STDLIB_PLAN.md`), requiring zero changes to the VM, Wasm, or native backends.

### 2. Scoped Resource Cleanup: `(defer expr...)`
With explicit allocators (`std/heap`, `std/arena`, `std/alloc`) and file descriptors, manual cleanup before every early `(return ...)` or `(try ...)` is error-prone.
* **`(defer expr...)`**: Registers `expr...` (must be `void`) to run at the exit of the enclosing lexical block (whether by normal fallthrough, `return`, `break`, or `continue`), in LIFO (reverse declaration) order.
* **Implementation:** Because AIPL traps abort immediately and recoverable errors are returned as values (`result`), `defer` has no stack-unwinding tables—it simply lowers to inlined cleanup statements at block exits and jump sites.

### 3. Fixing the Immortal Heap Problem (`str.Bytes` by Value & Scoped Strings)
Today, a long-running AIPL service (an HTTP server, daemon, or database) will eventually crash at the 2 GiB memory limit because:
1. `(ptr str.Bytes)` is a heap-allocated struct (`new Bytes`, 16 bytes on the never-freed bump heap) created on every `str.from_str`, `str.slice`, and `buf.bytes`.
2. Runtime `str` values (`str.concat`, `buf.to_str`, `(format "...")`) allocate from the never-freed bump heap.
3. `std/io` helpers like `io.print_int` call `(mem.alloc 11)` on every invocation.

**Revisions:**
1. **Make `str.Bytes` a `(value)` struct** (or introduce a built-in `(slice T)` value type carrying `(base_addr, offset, len, gen)`). Now that value structs (`VALUES_PLAN.md`) exist in every backend, passing and returning byte slices by value eliminates millions of immortal 16-byte allocations while keeping full bounds and generation checks.
2. **Use stack/scratch memory in `std/io`:** `io.print_int`, `io.print_i64`, and `io.print_f64` should format into a small buffer from `alloc.default` (and free it) or a thread-local scratch slot rather than calling `mem.alloc`.
3. **Add `buf.to_bytes` / arena-backed strings:** Encourage `(format_to buf "...")` and `str.Bytes` for dynamic text in loops, and reserve `str` for interned literals and permanent identifiers.

### 4. Eliminating the Top 4 Generator Syntax Frictions (Audit D3)
Keep "one obvious way to write each thing," but make that way match what models and programmers naturally write without off-by-one bugs:
1. **`(when cond body...)`**: A statement-only conditional (`void` body, no `else` branch), replacing the ~600 `(if cond body (block))` patterns across the repository.
2. **`(for i start end_excl body...)`**: Exclusive upper bound (`start <= i < end_excl`, step `1`), plus `(for_each x collection body...)` for `(arr T)` and `(ptr (vec.Vec T))`. Keep `(loop i start end_incl step ...)` for custom steps/inclusive ranges, or migrate to `for`.
3. **Variadic `(and a b c ...)` and `(or a b c ...)`**: Desugar `(and a b c)` into `(and a (and b c))` in the parser so 3+ conditions don't require manual right-nesting.
4. **Inferred union prefix in `match` arms:** When `v` in `(match v ...)` already has a known union type like `(option.Option (ptr (vec.Vec (ptr str.Bytes))))`, allow writing `(some [names] ...)` and `(none ...)` in addition to the fully qualified `((option.Option ...) some [names] ...)`.

### 5. `(derive ...)` and Generic Interface Implementations
* **`(derive cmp.Eq cmp.Ord cmp.Hash)`**: Allow structs, value structs, enums, and unions whose fields all implement `cmp.Eq` / `cmp.Ord` / `cmp.Hash` to automatically generate their implementation in the generics/interface pass.
* **Generic `impl` blocks:** Support `(impl (cmp.Eq (option.Option T)) (where (cmp.Eq T)) ...)` and `(impl (cmp.Hash (Pair A B)) (where (cmp.Hash A)) (where (cmp.Hash B)) ...)` with a strict orphan/non-overlap rule (one type constructor per interface).

### 6. Streaming `std/stream` (`Reader` and `Writer`)
Before or alongside `STDLIB_PLAN.md` L5–L7 (`deflate`, `csv`, `http`, TCP sockets), define a standard streaming abstraction in `std/stream`:
* `stream.Reader` and `stream.Writer` as value-struct vtables (holding a state handle + function references for `read` / `write` / `flush` / `close`, just like `alloc.Allocator` wraps allocators).
* Allows `json`, `csv`, `deflate`, `hash.sha256`, and `http` to stream directly over files, pipes, TCP sockets, and `buf.Buf` without buffering entire files in the 2 GiB address space.

---

## Part 3: Production OS, Networking, and Concurrency

### 1. Structured I/O Errors (`io.Error` Instead of `-1`)
* Replace raw `-1` WASI returns and `len = -1` sentinels in `std/io` and `std/fs` with `(result T io.Error)`, where `io.Error` is an enum (`not_found`, `permission_denied`, `already_exists`, `is_directory`, `not_directory`, `invalid_input`, `timed_out`, `broken_pipe`, `interrupted`, `unexpected_eof`, `other`).
* Preserve the underlying WASI errno in the VM, `aipl-run`, and native Linux syscalls (`aipl_src/native/wasi.aipl`) so programs can branch on *why* an operation failed.

### 2. Complete Filesystem Operations (`std/fs`)
Expand `std/fs` beyond `list` and `info`:
* **Directory & path mutations:** `fs.mkdir`, `fs.mkdir_all`, `fs.rmdir`, `fs.remove` (files), `fs.rename` (atomic file replacement—essential for safe config/database writes and package managers), `fs.symlink`, `fs.readlink`, `fs.chmod` (solving the `chmod +x` compiler bootstrap gap).
* **Streaming file handles:** `fs.File` with `open_read`, `open_write`, `open_append`, `read`, `write`, `seek`, `sync`, `close`.

### 3. Subprocesses & Signals (`std/proc`) in the L7 Host Interface
WASI preview 1 cannot spawn processes, which currently prevents AIPL from being used to write build tools, test harnesses, git wrappers, or agent orchestrators.
* When adding the `aipl` host namespace in `STDLIB_PLAN.md` Batch L7, include:
  * **`proc.run` / `proc.spawn`**: Execute a command with an explicit argv array, working directory, environment, stdin/stdout/stderr redirection or pipes, and optional timeout; gated by `--allow-run` in sandboxed mode.
  * **Signal handling (`os.on_signal`)**: Clean shutdown hooks for `SIGINT` / `SIGTERM` in servers and CLI tools.

### 4. TLS / HTTPS & I/O Multiplexing in Batch L7
* **TLS support:** Raw TCP (`STDLIB_PLAN.md` L7) is not enough for an HTTP client to call `https://` APIs (LLM endpoints, GitHub, package registries). Expose a host-backed TLS stream in the `aipl` host interface (`net.tls_connect`, `net.tls_accept`) backed by `rustls` in `aipl-run` and OS/linked TLS natively.
* **Socket timeouts & `net.poll`:** Add read/write deadlines and a `poll` primitive so an HTTP server does not require one 8 MiB OS thread per idle keep-alive connection.

### 5. Safe Message-Passing Concurrency (`std/chan` & Thread Pools)
AIPL already has `(where (plain T))` (built for `std/par`), which proves at compile time that a type `T` contains no mutable pointers or arrays at any depth.
* **Reuse `(where (plain T))` for Channels (`std/chan`):**
  * `(chan.Chan T)` with `(where (plain T))`: bounded, multi-producer/multi-consumer thread-safe queue built over `atomic.*` and futex wait/wake (`memory.atomic.wait32` / `notify`).
  * Because `T` is `plain`, sending `T` across threads copies only value data and immutable `str`s—**100% data-race-free concurrency for background workers, pipelines, and servers without needing `(unsafe ...)`!**

### 6. Native C FFI (`extern`)
* Add an `(unsafe)` foreign-function declaration form:
  `(extern "libsqlite3" (fn sqlite3_open [filename:i32 ppDb:i32] -> i32))`
* In `.wasm`, this lowers to a standard WebAssembly import (`"libsqlite3" "sqlite3_open"`).
* In the native backend, this binds via ELF dynamic linking (or `dlopen`/`dlsym` host trampolines), giving production users immediate access to SQLite, DuckDB, Postgres (`libpq`), and system libraries.

---

## Part 4: Compiler Speed, Targets & Tooling

### 1. Small-Function Inliner (Unlocks Loop Versioning Across Calls)
* **Why:** In [`docs/BENCHMARKS.md`](file:///home/matt/Projects/Repos/AIPL/docs/BENCHMARKS.md), `knucleotide` and `spectralnorm` pay heavily for tiny helper calls (`str.byte_at`, `vec.at`, matrix element functions) inside hot loops. Worse, the loop-versioning optimization (`GENERATIONS_PLAN.md`) cannot hoist null and generation checks out of a loop if the pointer access sits inside a helper function like `str.byte_at`.
* **What to build:** A simple AST-level inlining pass (in both toolchains, after type checking) for small, non-recursive leaf functions ($\le 12$ AST nodes). Once `str.byte_at` and `vec.at` are inlined into the caller's loop, the existing loop-versioning pass will automatically hoist their null, bounds, and generation checks to the loop preheader.

### 2. Complete the Self-Hosted CLI & Retire the Dual-Frontend Tax
* Add `verify`, `test`, `fmt`, and `context` subcommands to [`aipl_src/aipl.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl).
* Run `aipl test` and `aipl eval` by compiling to Wasm/native and executing in `wasmtime` / natively by default (keeping the slow Rust AST tree-walker `src/vm.rs` strictly for differential unit tests). This cuts test-suite runtime from minutes to seconds.

### 3. Native ELF Symbol Tables (`.symtab`) & ARM64 Backends
* **Profiler/Debugger support:** Emit a standard ELF `.symtab` and `.strtab` section in [`aipl_src/native/elf.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/elf.aipl) from the Wasm name section so `perf`, Flamegraphs, `gdb`, and `lldb` resolve AIPL function names natively.
* **Linux ARM64 & macOS Apple Silicon:** Add `arm64.aipl` (AArch64 instruction encoder) and `macho.aipl` alongside `x64.aipl` and `elf.aipl` in `aipl_src/native/`. Because the native backend translates Wasm rather than AIPL AST, each new target is self-contained (~2,500 lines of AIPL).

### 4. Built-In Formatter (`aipl fmt`) and Language Server (`aipl lsp`)
* **`aipl fmt`**: [`src/printer.rs`](file:///home/matt/Projects/Repos/AIPL/src/printer.rs) and `aipl_src/printer.aipl` already print canonical flat S-expressions. Add a deterministic pretty-printing mode (preserved comments, standard 2-space indentation by form kind) so agents and humans never argue over formatting.
* **`aipl lsp`**: A small Language Server written in AIPL itself (using `std/json` over stdin/stdout) wrapping `front.aipl` (`resolver` + `generics` + `checker`). Gives instant red squiggles, hover types, contract previews, and go-to-definition in VS Code, Cursor, and Neovim—and serves as a flagship real-world showcase written in AIPL.

<!-- --- -->

## Recommended Execution Order (After `STDLIB_PLAN.md`)

| Phase | Focus | Key Deliverables |
|---|---|---|
| **R1: Constraint Superpowers** | Make agents write better code with fewer round-trips | Unused var/param/result errors; `(pure)`, `(readonly)`, and `(no_bump)` clauses; inline `(example ...)` checks; `aipl verify --against <spec>`; `aipl context`; `aipl verify --fuzz` (contract fuzzer) |
| **R2: Language Ergonomics & Memory** | Scale to large files & long-running servers | `(try ...)` / `(try_opt ...)`; `(defer ...)`; `str.Bytes` as a `(value)` struct; `(when ...)`, exclusive `(for ...)`, variadic `and`/`or`; `(derive cmp.Eq cmp.Ord cmp.Hash)` |
| **R3: Production Systems & I/O** | Real OS errors, streaming, processes, safe channels | `io.Error` enum (preserve errno); `std/stream` (`Reader`/`Writer`); full `std/fs` (`mkdir`, `rename`, `chmod`, `File`); `std/proc` (subprocesses); TLS in L7; safe `std/chan` over `(where (plain T))` |
| **R4: Speed, Targets & Tooling** | Close the C gap & ship multi-platform tooling | AST leaf-function inliner; ELF `.symtab` for `perf`/`gdb`; `aipl fmt` & `aipl lsp` (written in AIPL); ARM64 native backend |

