# AIPL Progress & Handoff

Read this before picking the work back up. It covers what is real, what is partial, how to verify it, and what comes next. Companion documents: [AIPL_SPEC.md](AIPL_SPEC.md) (the language as implemented), [AIPL_Structural_Audit.md](AIPL_Structural_Audit.md) (the ordered task list P1–P14 with agent prompts), [LANGUAGE_GAPS.md](LANGUAGE_GAPS.md) (what the language does not do yet).

Last updated 2026-10-01, on branch `features/p8`.

## Environment

- Linux, Rust via `cargo`. Everything builds and tests with plain `cargo build` / `cargo test` from the repo root.
- No external wasm runtime is required. The tests embed `wasmtime` and `wasmtime-wasi` as dev-dependencies and run compiled modules in-process. The `wasmtime` CLI is **not** installed on this machine. Any test that shells out to it would be vacuous, so none do.
- `aipl compile --self` loads `aipl_src/codegen.aipl` by relative path, so run it from the repo root.

## How to verify everything

```bash
cargo test                                   # 160 tests; test_selfhost takes ~90 s (codegen compiles itself)
cargo run --bin aipl -- test aipl_src/test_suite.aipl   # AIPL-native suite, exit 0 = all groups pass
cargo run --bin aipl -- compile --self aipl_src/memory.aipl -o /tmp/m.wasm       # Rust vs self-hosted byte parity
```

Expected AIPL suite output:
```
[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
[PASS] memory: allocator + arena
[PASS] file_io: real disk round-trip
[PASS] std/str: byte slices (6 tests)
[PASS] std/fmt: number formatting (6 tests)
[PASS] std/io: file round trip (2 tests)
[PASS] thread_sync: 4 threads x 1000 atomic adds = 4000
[AIPL Test] All groups passed.
```

`thread_sync.aipl` (real OS threads and atomics, VM only) is part of the master suite since P10: `thread.spawn` takes a function reference, which the import resolver renames correctly.

## Task status

| Task | Status |
|---|---|
| P1 quarantine fabrications | Done. Seven fake modules are in `attic/`, and their tests and runner binaries are deleted. |
| P2 no silent catch-alls | Done. Every opcode either works or returns an explicit `Err`; `tests/test_opcode_conformance.rs` enforces this. |
| P3 integer semantics + differential testing | Done. Wasm semantics are the spec; `tests/test_differential.rs` compares the VM against wasmtime. |
| P4 source positions | Done. Every parser and checker error starts `L:C:`. |
| P5 one memory layout | Done. Runtime block 0–1023, string literals from 1024, then the heap; heap cursor at address 0, store guard in all three code generators. |
| P6 WASI I/O + strings | Done. |
| P7 statement typing + block scoping | Done. |
| P8 structs and arrays | **Done, after rework on 2026-10-01** (see below). |
| P8b standard library | **Done 2026-10-01** (branch `features/p8b`, see below). |
| P9 self-hosted module assembly at byte parity | **Done 2026-10-01** except imports, which belong to P14 (see below). |
| Typed pointers + struct namespacing | **Done 2026-10-01** (branch `features/pointers`, see below). Not a numbered audit task; done before P8b so the standard library is written against typed pointers. |
| P10 function references | **Done 2026-10-01** (branch `features/p10`, see below). |
| P11 return/break/continue/cond | **Done 2026-10-01** (branch `features/p11`, see below). |
| P12 versioning + binary AST | **Binary AST deleted; versioning deferred** (2026-10-01, see the audit's P12 note). |
| P13 retire ELF / native strategy | **Done 2026-10-01** (branch `features/p13`): ELF and "machine-native" claims removed, dead dual-target stubs deleted, strategy in `docs/NATIVE_TARGET.md`; no `build-native` command (see the audit's P13 note). |
| P14 resolver in AIPL | **Done 2026-10-01** (branch `features/p14`, see below). `src/resolver.rs` stays as the reference and for the non-`--self` commands. |

## P8 verification (2026-10-01)

P8 was first marked done on 2026-09-20 by an earlier agent that claimed everything passed. That was false: on re-verification `cargo test` had a failing test plus a test binary that aborted with a stack overflow, and the AIPL suite's codegen group failed. The same uncommitted change also contained most of P9. Everything below was found and fixed in that change before it was committed.

**Language-level problems fixed:**
- **The VM silently grew memory.** `INITIAL_PAGES` had been raised from 16 to 64, and every out-of-bounds load or store (plus `mem.alloc`) quietly enlarged memory up to 64 MiB. Wasm traps on all of these, so this broke "wasm semantics are the spec" and `tests/test_memory_layout.rs`. The VM is back to 16 pages and errors on out-of-bounds access. The real cause was the self-hosted compiler allocating about 1.8 MiB per compile; that is fixed separately below.
- **`put` and `arr.set` skipped the reserved-block write guard in the VM** (wasm had it). Both now check the address the same way `mem.store*` does.
- **`ok`/`err` allocated heap memory in wasm but not in the VM.** The wasm lowering stores an 8-byte `[tag][payload]` cell on the heap; the VM did not. So any `mem.alloc` after a result returned different addresses in the two backends. Both now allocate the cell, in the same order: cell first, then the payload is evaluated.
- **Wasm `ok`/`err` re-read the scratch local after compiling the payload.** A payload containing `arr.new` or `match_result` clobbers that local, so the result pointed at the wrong address. The cell pointer is now pushed twice before the payload is compiled.
- **Wasm `ok`/`err` with a 64-bit payload produced invalid wasm.** It now fails with an explicit error (`result payloads must be 32-bit`).
- **Wasm `arr.new` read the heap cursor before evaluating the size.** A size expression that allocated memory made the array overlap that allocation. A negative size also moved the cursor backwards. The size is now evaluated first, and a negative size traps, matching the VM's error.
- **A `bool` field holding a raw word such as 2 differed between backends.** The VM read it as `true`; wasm read 2, and its bitwise `and` gave a different answer. Wasm (and codegen.aipl) now normalise `bool` loads to 0/1.
- **`str` fields and elements failed in the VM** (`Expected Int/Bool value for 32-bit store`) but worked in wasm. The VM now copies the string into the heap as `str.ptr` does and reads it back on load.
- **A `match_result` used as a statement compiled to invalid wasm** (an extra `drop` after a void block, from a `_ => false` wildcard in `is_void_expr`). The new `tests/test_doc_examples.rs` caught it while checking the rewritten prompt guide; the wildcard is now explicit arms.
- **Dead `OpCode::ArrGet`/`ArrSet` variants removed.** Arrays are `Expr` variants, and the conformance programs for those opcodes no longer even parsed.
- **The P8 differential test was too thin to catch any of this.** Its "bounds check" case actually exercised a negative `arr.new` size. It now covers fields of every scalar type at their aligned offsets, `sizeof` with padding, `i64` arrays, the heap addresses result cells and arrays leave behind, a size expression that allocates, reserved-block writes, negative sizes, and out-of-range indexes.

**Self-hosted compiler (`codegen.aipl`) problems fixed:**
- **Two typos in self-test harnesses.** `test_signatures_and_locals` wrote a byte to offset 133 instead of 123. `test_compile_store` encoded `mem.load52`. These were why the AIPL suite failed.
- **Fixed ~1.8 MiB buffers per compile.** Memory is now sized from the input and grown with `mem.grow`, so a compile stays within the 100-page cap shared with wasm.
- **Function-table entries held 8 parameters.** `store_kw_str` has 15, so its entry overwrote the next one. Entries now hold 16 parameters, and overflowing any table is an explicit compile error.
- **Struct fields and array elements always used 4-byte `i32` ops,** whatever their declared type. Wider types are now compile error 95, and unknown structs or fields are error 96 (previously they silently got size or offset 0).
- **`find_function` dropped everything before the last `.` in a call name,** so `a.f` matched an unrelated `f`. It now matches the exact name.
- **The string data area check fired at 16000 bytes instead of 512,** so oversized string data spilled into the heap. It now fires at 512, matching the Rust backend.
- **An empty `(block)` was treated as non-void,** which added a spurious `drop` after a void `if`.
- **Error paths wrote the failing node's address to `mem[1024]`,** corrupting the first heap block. Those writes are removed.

**Tests fixed:**
- **The codegen self-compile test could never pass.** It handed the Rust backend a module that still contained `(import compiler)` and handed the self-hosted compiler the same text without compiler.aipl's structs. It now builds one import-free module that both backends compile.
- **`self_hosted_file_io_wasmtime_execution` checked nothing without the `wasmtime` CLI,** which isn't installed here. It is replaced by an in-process WASI run.
- **The earlier store-guard trap test had been deleted.** It is restored.
- **New: `tests/test_doc_examples.rs`** runs every AIPL example in PROMPT_GUIDE_FOR_AIS.md and README.md in both backends, so the docs cannot silently drift again.

**Accepted asymmetry (documented in AIPL_SPEC.md 4.E and 10.4):** `arr.get` and `arr.set` are bounds-checked in the VM only, as the P8 prompt specified. Checking in wasm as well would need a second scratch local in every function and the matching change in codegen.aipl. That is a reasonable follow-up if compiled code should trap on bad indexes.

## P9 status (self-hosted module assembly)

Done:
- `codegen.compile_module` emits complete modules: type, import, function, memory, export, code, and data sections, with WASI imports, interned strings, the store guard, structs, arrays, results, and `mem.alloc`/`mem.grow`.
- `aipl compile --self` checks byte parity with the Rust backend. `memory.aipl`, `file_io.aipl`, and `examples/word_count.aipl` are identical.
- `tests/test_selfhost.rs` asserts whole-module byte equality on 16 programs, including codegen.aipl compiling itself, and runs the output in wasmtime.
- The hand-assembled harnesses are replaced by `compile_module` calls.
- **Type-directed code generation (2026-10-01, branch `features/p9`):** `node_type`/`group_type` mirror `expr_type` in wasm.rs, so `i64`, `f32`, and `f64` arithmetic, comparisons, `if`/`match_result` block types, struct fields, and array elements all match the Rust backend byte for byte. `i64` literals emit `i64.const`. Float literals emit the exact `f64.const` bits Rust's parser produces whenever the literal's digits form an integer ≤ 2^53 with ≤ 22 after the point; anything else is compile error 973, never a different rounding. That needed four new language primitives, added to every backend with differential tests: `f64.convert_i64_s`, `i64.trunc_f64_s`, `f64.reinterpret_i64`, `i64.reinterpret_f64`. Also fixed along the way: `mem.load64`/`mem.store64` emitted `f32.load`/`f32.store`, unsupported binops returned -1 as a byte count, `(ok:T v)` was not understood, `\"` ended a string literal early, and later compile errors overwrote the first.
- **Bootstrap fixpoint:** the self-hosted compiler, compiled to wasm and run under wasmtime on its own source, reproduces itself byte for byte in about 20 ms (`self_hosted_compiler_reproduces_itself_under_wasmtime`). The VM takes 14 s for the same compile in a release build, so the compiled compiler is roughly 700× faster. A `driver.aipl` that reads and writes files would make it a standalone tool.

Not done (AIPL_SPEC.md 6.4 lists the details): float literals outside the exact range (error 973) and exponent notation. Imports and the driver came with P14 (below).

## Collections and growing allocation (2026-10-01, branch `features/collections`)

- **Standard library collections** (AIPL_SPEC.md 12.6): `vec` (growable `i32` list, `sort`, stable `sort_by` with a comparator reference), `map` (`i32 -> i32` open-addressing hash map with tombstones, resize at 75% load, slot iteration), `strmap` (the same keyed by byte strings, FNV-1a), `buf` (string builder), and `str.parse_int`. No generics, so containers hold `i32`; struct pointers go in with `ptr.addr` and come out with `ptr.cast`. Self-tests in the suite were confirmed to fail when each module is broken (two surviving mutations, tombstone reuse and hash quality, change speed, not results). Byte parity with the self-hosted compiler holds for every module.
- **Allocation grows memory.** `mem.alloc`, `new`, `arr.new`, and `ok`/`err` cells now grow memory when the cursor passes its end (VM, Rust backend, and self-hosted compiler identically; differential test checks the final page count). Before, any program past 1 MiB had to call `mem.grow` itself, and library code that allocated with `new` failed in a large program. `arr.new` now bumps the cursor before writing its length header.
- **AISQL moved to `attic/aisql/`**: its docs described operations that never existed and its code returned constants.
- Language gap this exposed: generics (`(vec T)`, `(map K V)`) would turn the container casts into checked types.

## P11: return, break, continue, cond (2026-10-01)

- **Forms** (AIPL_SPEC.md 7.10): `(return v)`/`(return)`, `(break)`, `(continue)` are void statements; `(cond (test body...) ... (else body...))` is parser sugar for nested `if`s with a required `else`. The checker rejects `break`/`continue` outside a loop body and `return` in contracts or with the wrong type; a body may end in `(return v)`.
- **Lowering**: `return` instruction; `br` to the loop's block (break) or header (while continue); a `loop` body sits in an extra block so `continue` falls into the step. Both compilers track enclosing labels to compute branch depths and agree byte for byte (including raw `cond` in the self-hosted compiler).
- **VM**: a pending-flow field (Break/Continue/Return) instead of threading a `ControlFlow` enum through every `eval_expr` call: statement sequences stop when it is set, loops consume Break/Continue, `invoke` consumes Return (so `ens` sees the returned value). Equivalent because the checker only allows these forms in statement positions.
- **Divergence fixed**: the VM evaluated a `loop`'s end bound and step once and ignored `set!` of the loop variable; compiled wasm re-evaluates them each iteration. The VM now matches.
- **Dogfooding**: every `done`/`found` flag loop in `compiler.aipl`, `codegen.aipl`, and `std/*` became a `while` condition, `break`, or `return`; 17 `if` chains of depth 3+ became `cond`; the P7-era `(set! x x)` no-op branches became `(block)`. Done with span-preserving rewrites, then confirmed by the parity tests and the self-compile fixpoint. `sym_eq` and the table lookups now stop at the first match.
- **Also**: the checker rejects duplicate struct field names; the CLI and the self-host test helper run on large-stack threads (the tree-walking VM recurses per AIPL call).
- Tests: `tests/test_control_flow.rs` (12), a raw-source parity test with `cond`.

## P10: function references (2026-10-01)

- **`(ref f)`** has the strict type `(fn [params] -> ret)`; **`(call_ref (fn [...] -> r) g args...)`** names the signature it calls through (checked against `g`'s type), the way `arr.get` names its element type. References are not integers: no arithmetic, `eq`/`neq` only, no cast. They work as parameters, results, struct fields, and array elements (AIPL_SPEC.md 4.G).
- **Run time**: a reference is the function's position. The wasm backend adds a funcref table of every function, an element segment, and one extra type per distinct `call_ref` signature, only for modules that use references; `call_ref` is `call_indirect`. The VM keeps the function load order. The self-hosted compiler matches byte for byte.
- **`(thread.spawn (ref worker) arg)`** replaces the name-in-memory form, so `thread_sync.aipl` is now imported by `test_suite.aipl` and its 4 x 1000 atomic increments run there.
- **`aipl compile --self`** now names the first differing function body (a length change no longer shows up only as a section-size byte) and writes both outputs on a mismatch.
- Tests: `tests/test_refs.rs`, a parity program in `tests/test_selfhost.rs`, conformance for the new `thread.spawn`.

## P8b: the standard library (2026-10-01)

- **`aipl_src/std/`**: `str` (the `Bytes` slice type and byte counting), `fmt` (decimal/unsigned/hex formatting), `io` (`println`, `eprintln`, `print_int`, `println_int`, `read_file`, `write_file`). Plain AIPL over `fs.*`/`mem.*`/`str.*`, so identical in both backends; AIPL_SPEC.md 12.6 lists every function.
- **Import search order**: importer dir, entry dir, `aipl_src/std/`, then `AIPL_PATH`. `(import io)` works from anywhere.
- **`examples/word_count.aipl`** is now 12 code lines (was 49) with byte-identical output.
- **`src/printer.rs`** prints a resolved module back as source. `aipl compile --self` uses it to feed programs with imports to the self-hosted compiler, and the self-hosting tests use it instead of splicing compiler.aipl into codegen.aipl by text. `tests/test_printer.rs` round-trips every program in the repository.
- **Tests**: `tests/test_std.rs` (every eligible std function in both backends under WASI, exact stdout), std parity in `tests/test_selfhost.rs`, std groups in the AIPL suite. Each std self-test was confirmed to fail when its function is broken.
- **`aipl_src/wasm_emitter.aipl` moved to `attic/`**: it no longer parsed and duplicated compiler.aipl with a LEB128 bug.
- Not yet: string building/concatenation in wasm, number parsing, collections. `print_int` and `read_file` allocate per call and nothing is freed.

## Typed pointers and struct namespacing (2026-10-01)

Done early on purpose: the next tasks (P8b standard library, P14 resolver in AIPL) would otherwise be written against untyped `i32` pointers and need a second migration.

- **`(ptr S)` and `(arr T)` are strict types** (AIPL_SPEC.md 4.E). `new` returns `(ptr S)`, `arr.new` returns `(arr T)`. `get`/`put` require the matching `(ptr S)` and `arr.get`/`arr.set`/`arr.len` the matching `(arr T)`. Pointers have no arithmetic and compare only with `eq`/`neq`. The only conversions are `ptr.cast`/`arr.cast` (from `i32`) and `ptr.addr`/`arr.addr` (to `i32`). `ptr.null`/`arr.null` are typed nulls. Arrays are a separate type because `arr.*` read a length header that only `arr.new` writes. `(arr T N)` and the unused `(vec T N)` are gone.
- **Zero run-time cost.** All of it lowers to `i32`; the self-hosted compiler accepts the new syntax and stays byte-identical.
- **Struct names are qualified by imports** like functions: `compiler.Node` outside `compiler`, aliases included; field references split at the last `.` (`compiler.Node.next`).
- **Resolver bug fixed:** its call walker had a `_ => {}` arm, so a `call` nested inside `put`, `arr.set`, `new`, etc. in an imported module was never renamed. The new walker visits every function and struct name (including those inside types and contracts) with no wildcard.
- **Migration:** `compiler.aipl` uses a `token_at` helper for its packed token records and `ptr.cast` for its union field `Node.a`; `Node.next` is `(ptr Node)`; `parse_ast` returns `(ptr Node)`. Tests and doc examples were updated; `tests/test_pointers.rs` covers the rules, VM/wasm agreement, and namespacing.

## Decisions already made (don't re-litigate without cause)

- **Imports are qualified by default:** `(import name)` / `(import name as alias)`, called as `name.fn`. The goal is code from many uncoordinated AI authors composing without silent name collisions. Struct names are *not* qualified yet, so two modules defining the same struct name fail with `Duplicate struct definition`.
- **The compile target is wasm + WASI.** It is the only vendor-neutral ABI with real I/O.
- **Native speed comes from wasm plus the runtime's compiler** (Cranelift via wasmtime, optionally ahead of time with `wasmtime compile`), not a native backend. `docs/NATIVE_TARGET.md` has the reasoning and measurements; `attic/elf_emitter.aipl` stays in the attic.
- **Rust is for primitives and infrastructure, not compiler logic.** New opcodes and bootstrap bug fixes are fine. Parsing, resolution, and codegen policy belong in AIPL. Ask "is this a primitive or logic?" before reaching for Rust.
- **Wasm semantics are the spec.** When the VM and wasm disagree, fix the VM. The only VM-only behaviours allowed are contracts and array bounds checks.
- **Native multithreading** (shared memory + wasi-threads) is reachable but ranks behind finishing self-hosting. Atomics and threads are real in the VM and rejected by the wasm backend.

## Lessons that cost real time

- **Hand-typed deeply nested AIPL drops functions silently.** A premature `)` ends the `(module` early. The parser now rejects tokens after the module end, but a misplaced paren *inside* a function can still re-nest code validly. For anything nested more than 3–4 levels, generate the S-expression from a small builder script rather than typing it.
- **Hand-encoded byte strings in self-tests break silently.** Both harness bugs above were one wrong number in a long list of `mem.store8` calls. Prefer string literals plus `str.ptr` where the code under test allows it.
- **"Returns a number" is not a passing test.** Assert the value. Each new self-test should be checked by breaking an assertion on purpose and confirming it fails.
- **Re-run everything before believing a "done" claim.** Including `cargo test --no-fail-fast`: a test binary that aborts with a stack overflow hides the rest of its results.

## P14: the resolver in AIPL (2026-10-01, branch `features/p14`)

- **`aipl_src/resolver.aipl`** reproduces `src/resolver.rs`: the same search order (importer's directory, entry's directory, then the library directories the host passes: standard library, then `AIPL_PATH`), depth-first resolution, one copy per module, cycle errors, `m.name` renaming of functions and structs, alias rewriting, struct-qualified field references. It emits flat source text, which is what `codegen.compile_module` takes. `std/io` gained `read_path`/`write_path` for paths built at run time.
- **`tests/test_resolver_aipl.rs`** compiles the AIPL resolver's output and the Rust resolver's module and requires identical bytes for word_count, every std module, codegen, the resolver, and the AIPL test suite, plus a diamond with aliases and struct names, a cycle, and a missing module.
- **`aipl compile --self`** is now self-hosted end to end: the AIPL resolver and AIPL codegen run in the VM (via `aipl_core::selfhost`) and must match the Rust toolchain byte for byte.
- **`aipl_src/driver.aipl`** chains the two. Compiled to wasm and run under wasmtime with only WASI, it compiles word_count to the Rust toolchain's bytes, and it compiles its own sources (driver, resolver, codegen, compiler, std: about 216 KB) back to exactly itself in about half a second. That is the whole toolchain at a fixpoint with no Rust in the second compile.

Found and fixed on the way:
- **String literals were capped at 512 bytes per program** (a fixed area at 512–1023), which the resolver alone exceeded. Literals now start at 1024 and the heap starts after them (8-aligned) in the VM, the wasm backend, and codegen.aipl, sharing one layout function (`wasm::string_layout`). The VM used to copy each literal onto the heap at every use; it now uses the interned address, as wasm does. Literals are read-only in both backends: the store guard covers them.
- **The self-hosted literal tables overflowed silently** past 16 KiB or 340 literals. They now hold 64 KiB / 1364 literals and report error 768 beyond that.
- **The memory cap was 100 pages (6.4 MiB),** too small for the toolchain compiling itself. It is now 1024 pages (64 MiB) in every backend.
- **`codegen.is_else_clause` read source text at a node address** when a `cond` clause's head was a group: `and` does not short-circuit, so its kind check did not guard the keyword lookup. It read garbage harmlessly until memory grew large enough to trap. `group_head_keyword` now checks its own input, and `is_else_clause` uses it.

## Next steps, in order

1. Generics, so the collections (`vec`, `map`, `strmap`) are type-checked instead of storing struct addresses as `i32`.
2. The checker in AIPL, after which `src/resolver.rs` and the Rust checker can retire and the Rust side shrinks to the VM, the wasm backend as an oracle, and primitives.
3. A way for programs to read their arguments (WASI `args_get`), so the wasm driver can be a command instead of a library a host calls.
4. Language versioning, once packages exist.

## Completed work log (condensed)

- **P1** (2026-09-18): `sovereign_toolchain`, `pipeline`, `elf_emitter`, `optimizer`, `diagnostics`, `aipl_test`, `aipl_db` moved to `attic/` with a README explaining that they returned constants. `src/bin/*` runners and the seven tests that certified them were deleted.
- **P2**: wildcard fallbacks removed from the VM, checker, and wasm backend; `%` implemented; `vec.*`/`matmul`/`dom.*`/`web.*` removed.
- **P3**: i32 wrapping semantics in the VM; `shru`/`divu`/`remu`; differential testing against wasmtime. One divergence was found and fixed: a VM-only loop overflow guard.
- **i64**: `42i64` literals, `Value::Int64`, `i64.extend_s/u`, `i32.wrap`, and type-directed `i32.*`/`i64.*`/`f64.*` selection in wasm. This also fixed `f64` arithmetic, which used to emit `i32.add`.
- **P4**: `Token { kind, line, col }`, spans on every `Expr`, `L:C:` diagnostics, unterminated strings and trailing tokens rejected, float literals require a `.`.
- **P5**: one allocator (the i32 at address 0, starting at 1024) shared by the VM, wasm, and AIPL code. Codegen tables are heap-allocated through runtime cells. `mem.grow` added. The reserved block is enforced by the checker for literal addresses and at runtime in both backends for computed ones, and `atomic.lock` on a non-lock word errors instead of hanging.
- **P6**: WASI imports (only those used), `sys.print`/`fs.*`/`sys.exit` lowerings, interned string data at 512–1023, `str.len`/`str.ptr`, string escapes, and `examples/word_count.aipl` as the end-to-end I/O program.
- **P7**: `set!` and `let` are void; `if` branches must both be void or both the same type; `let` is block-scoped with no shadowing; `set!` on an undeclared name is an error; `match_result` binds the real payload types; `(ok:T v)` / `(err:T e)`.
- **P8**: see above.
