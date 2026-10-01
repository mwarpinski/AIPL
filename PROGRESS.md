# AIPL Progress & Handoff

Read this before picking the work back up. It covers what is real, what is partial, how to verify it, and what comes next. Companion documents: [AIPL_SPEC.md](AIPL_SPEC.md) (the language as implemented), [AIPL_Structural_Audit.md](AIPL_Structural_Audit.md) (the ordered task list P1–P14 with agent prompts), [LANGUAGE_GAPS.md](LANGUAGE_GAPS.md) (what the language does not do yet).

Last updated 2026-10-01, on branch `features/p8`.

## Environment

- Linux, Rust via `cargo`. Everything builds and tests with plain `cargo build` / `cargo test` from the repo root.
- No external wasm runtime is required. The tests embed `wasmtime` and `wasmtime-wasi` as dev-dependencies and run compiled modules in-process. The `wasmtime` CLI is **not** installed on this machine. Any test that shells out to it would be vacuous, so none do.
- `aipl compile --self` loads `aipl_src/codegen.aipl` by relative path, so run it from the repo root.

## How to verify everything

```bash
cargo test                                   # 120 tests; test_selfhost takes ~90 s (codegen compiles itself)
cargo run --bin aipl -- test aipl_src/test_suite.aipl   # AIPL-native suite, exit 0 = all groups pass
cargo run --bin aipl -- eval aipl_src/thread_sync.aipl --func run_thread_tests   # standalone; prints Int(1) (pass count), see below
cargo run --bin aipl -- compile --self aipl_src/memory.aipl -o /tmp/m.wasm       # Rust vs self-hosted byte parity
```

Expected AIPL suite output:
```
[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
[PASS] memory: allocator + arena
[PASS] file_io: real disk round-trip
[AIPL Test] All groups passed.
```

`thread_sync.aipl` is excluded from the master suite on purpose. `thread.spawn` finds its target by a name string stored in linear memory, which the import resolver cannot rewrite. Once imported, `worker_increment` becomes `thread_sync.worker_increment` and the spawn fails. P10 (function references) fixes this.

## Task status

| Task | Status |
|---|---|
| P1 quarantine fabrications | Done. Seven fake modules are in `attic/`, and their tests and runner binaries are deleted. |
| P2 no silent catch-alls | Done. Every opcode either works or returns an explicit `Err`; `tests/test_opcode_conformance.rs` enforces this. |
| P3 integer semantics + differential testing | Done. Wasm semantics are the spec; `tests/test_differential.rs` compares the VM against wasmtime. |
| P4 source positions | Done. Every parser and checker error starts `L:C:`. |
| P5 one memory layout | Done. Runtime block 0–1023, heap cursor at address 0, store guard in all three code generators. |
| P6 WASI I/O + strings | Done. |
| P7 statement typing + block scoping | Done. |
| P8 structs and arrays | **Done, after rework on 2026-10-01** (see below). |
| P8b standard library | Not started. |
| P9 self-hosted module assembly at byte parity | **Done 2026-10-01** except imports, which belong to P14 (see below). |
| P10–P14 | Not started. |

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

Not done (AIPL_SPEC.md 6.4 lists the details):
- **No import resolution.** `compile_module` takes one module; the parity and fixpoint tests merge compiler.aipl into codegen.aipl by hand. This is P14's job (resolver in AIPL).
- **`compile_to_target` in `compiler.aipl` still returns -1.** Wiring it needs a third driver module (the P9 prompt explains why). A `driver.aipl` that reads a file, calls `compile_module`, and writes the result would also make the compiled self-hosted compiler a standalone command-line tool.
- Float literals outside the exact range (error 973) and exponent notation.

## Decisions already made (don't re-litigate without cause)

- **Imports are qualified by default:** `(import name)` / `(import name as alias)`, called as `name.fn`. The goal is code from many uncoordinated AI authors composing without silent name collisions. Struct names are *not* qualified yet, so two modules defining the same struct name fail with `Duplicate struct definition`.
- **The compile target is wasm + WASI.** It is the only vendor-neutral ABI with real I/O.
- **Native speed comes from wasm plus an external AOT compiler** (Cranelift via wasmtime), not a hand-written ELF backend. `attic/elf_emitter.aipl` stays in the attic (P13).
- **Rust is for primitives and infrastructure, not compiler logic.** New opcodes and bootstrap bug fixes are fine. Parsing, resolution, and codegen policy belong in AIPL. Ask "is this a primitive or logic?" before reaching for Rust.
- **Wasm semantics are the spec.** When the VM and wasm disagree, fix the VM. The only VM-only behaviours allowed are contracts and array bounds checks.
- **Native multithreading** (shared memory + wasi-threads) is reachable but ranks behind finishing self-hosting. Atomics and threads are real in the VM and rejected by the wasm backend.

## Lessons that cost real time

- **Hand-typed deeply nested AIPL drops functions silently.** A premature `)` ends the `(module` early. The parser now rejects tokens after the module end, but a misplaced paren *inside* a function can still re-nest code validly. For anything nested more than 3–4 levels, generate the S-expression from a small builder script rather than typing it.
- **Hand-encoded byte strings in self-tests break silently.** Both harness bugs above were one wrong number in a long list of `mem.store8` calls. Prefer string literals plus `str.ptr` where the code under test allows it.
- **"Returns a number" is not a passing test.** Assert the value. Each new self-test should be checked by breaking an assertion on purpose and confirming it fails.
- **Re-run everything before believing a "done" claim.** Including `cargo test --no-fail-fast`: a test binary that aborts with a stack overflow hides the rest of its results.

## Next steps, in order

1. Commit the P8 rework (this branch).
2. A `driver.aipl` so the compiled self-hosted compiler runs as a standalone tool under any WASI host.
3. P8b standard library (needs P11 per the audit's sequencing note), P10 function references (fixes `thread_sync` under imports), P11 `return`/`break`/`continue`/`cond`.
4. P12–P14 per the audit.

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
