# AIPL Progress & Handoff

Session log and continuation point for the self-hosting effort. Read this
before picking the work back up — it says exactly what's real, what's still
stubbed, and what to do next. Companion to [LANGUAGE_GAPS.md](LANGUAGE_GAPS.md)
(the detailed gap analysis + inline progress notes) and
[AIPL_SPEC.md](AIPL_SPEC.md) (the language spec, now slightly corrected).

## Environment setup (do this first in a fresh session)

Nothing here can be built or tested without these two things, and neither
was installed by default:

1. **Rust is installed inside WSL, not on Windows directly.** Cargo isn't on
   the Windows PATH. Run all `cargo`/`rustc` commands via:
   ```
   wsl bash -lc "cd /mnt/c/Users/MattWarpinski/open_source_project/AIPL && cargo <...>"
   ```
   If `cc`/linker errors show up, `build-essential` may need reinstalling —
   it was installed via `wsl -u root apt-get install -y build-essential`
   (bypasses `sudo`, which needs a password that isn't the Windows login
   password).

2. **`wasmtime` (Python bindings) is the WASM validator/runtime** used to
   actually load and call compiled `.wasm` files, since no other WASM runtime
   (Node, wasmtime CLI, browser) is available here. Installed via:
   ```
   "C:\Users\MattWarpinski\AppData\Local\Programs\Python\Python312\python.exe" -m pip install wasmtime
   ```
   Use it to `Module.from_file(...)` + `Linker(...).instantiate(...)` any
   newly compiled `.wasm` — this is how every wasm-side bug in this log was
   actually caught, not just theorized about.

3. **`wsl bash -lc "...; echo $?"` does not reliably report a subprocess's
   real exit code in this sandboxed shell** — verified with a bare
   `std::process::exit(1)` reporting `0` through the Bash tool, then
   confirmed as `1` through PowerShell (`wsl <binary>; $LASTEXITCODE`) for
   the same binary. If you need to check an `aipl` exit code, use PowerShell,
   not `wsl bash -lc`. Don't mistake this for an actual bug in `aipl test`'s
   exit-code logic — the logic itself is correct.

## Terminal-facing test runner (new)

`aipl test <file> [--func run_all]` runs an AIPL entrypoint and turns its
return value into the process exit code: the entrypoint must return an `i32`
failure count (`0` = everything passed), and all pass/fail reporting happens
in AIPL via `sys.print`, not in Rust — Rust's role is just parse, check,
invoke, translate the result to an exit code. This is what "run the tests
using AIPL from the terminal" means today, short of the full self-hosting
goal (see "What `aipl test` is not" below).

`aipl_src/test_suite.aipl` is the master suite: it `(import ...)`s every
module with a genuinely verified self-test (currently `compiler`, `memory`,
`file_io`) and aggregates their results. Run it with:
```
aipl test aipl_src/test_suite.aipl
```
Add a new module to it only once its own self-test has been verified for
real (not just "returns 1") — see `LANGUAGE_GAPS.md`'s lesson on that.

**`thread_sync.aipl` is deliberately excluded from the master suite** — a
real bug, not an oversight. `thread.spawn` finds its target function by
reading a name string out of *linear memory at runtime*; the import resolver
only rewrites function names that appear as `(call ...)` *syntax*, so it has
no way to see or rewrite that in-memory byte string. Once imported,
`worker_increment` gets renamed to `thread_sync.worker_increment` and the
spawn fails with "unknown function". Run that module's test standalone:
`aipl test aipl_src/thread_sync.aipl --func run_thread_tests`. Fixing this
for real needs either a naming convention that survives import rewriting or
first-class function values — see `LANGUAGE_GAPS.md`.

**What `aipl test` is not**: the `aipl` binary is still Rust hosting a VM
(parse → check → interpret). "Run tests using AIPL, not Rust" is true of the
test *logic* now, not of the interpreter itself — that's the much bigger,
already-tracked self-hosting goal (real codegen, then a native/WASI runner),
not a few-days task. Don't conflate the two when reporting status.

## Git state

Branch: `features/wasm_runtime`. Two commits already made this session
(`3474ecf`, `2d15316`) contain most of the work below. **Three files are
currently staged but not committed**: `LANGUAGE_GAPS.md`,
`aipl_src/compiler.aipl`, `src/resolver.rs` (the `parse_ast` work + the
resolver's "temporary scaffolding" doc comment + gap-doc updates). Diff vs.
`origin/master`: 14 files, +1539/-197.

### Quarantining Fabrications (P1 Audit Task)
- **Moved 7 fabricated `.aipl` modules to `attic/`**: `sovereign_toolchain.aipl`, `pipeline.aipl`, `elf_emitter.aipl`, `optimizer.aipl`, `diagnostics.aipl`, `aipl_test.aipl`, and `aipl_db.aipl` were quarantined into `attic/`.
- **Created `attic/README.md`**: Explains that these modules return hardcoded constants instead of performing work, citing specific instances (`sovereign_toolchain.aipl` `fs_open` returning 10/11, `fs_read` returning count, `thread_spawn_sync` returning 101, `tokenize` counting parens; `pipeline.aipl` `pipeline_tokenize` returning 4 on zero tokens; `compiler.aipl` `emit_wasm_binary` reading opcode from `ast_ptr+12`).
- **Deleted unused runner binaries**: `src/bin/aipl_test_runner.rs` and `src/bin/aisql_runner.rs` were removed along with their `[[bin]]` sections in `Cargo.toml`.
- **Cleaned `tests/test_v2.rs`**: Deleted 7 tests that certified attic modules or stubbed compiler functions.
- **Cleaned `aipl_src/compiler.aipl`**: Removed `emit_wasm_binary`, `aipl_heap_alloc` (duplicate of `memory.aipl`), and the ELF branch from `compile_to_target` (which now returns `-1` with a comment `;; not yet wired to codegen.aipl`).

### Eliminating Silent Fallbacks & OpCode Conformance (P2 Audit Task)
- **Eliminated 4 silent fallbacks**: Removed `_ => Ok(Value::Int(0))` in `vm.rs`, `_ => Nop` (both Op and Expr matches) in `wasm.rs`, `_ => Ok(Type::I32)` in `checker.rs`, and `_ => ValType::I32` in `aipl_to_wasm_type`.
- **Implemented `OpCode::Mod`**: In VM (`%` with div-by-zero check) and WASM (`I32RemS`).
- **Implemented `MemAlloc` in WASM**: Dynamic bump allocator using a WASM global (index 0, `ValType::I32`, mutable, initialized to 1024 via `GlobalSection`).
- **Configured `SysPrint` in WASM**: Returns an explicit `Err` ("comes with WASI in P6").
- **Removed 7 unsupported OpCodes**: `VecDot`, `MatMul`, `DomElem`, `DomMount`, `DomAppend`, `DomOnEvent`, `WebAlert` removed from `ast.rs`, `parser.rs`, `checker.rs`, and `AIPL_SPEC.md`.
- **Added `tests/test_opcode_conformance.rs`**: Conformance test powered by `wasmparser` verifying every remaining OpCode satisfies option (a) or option (b).

### Establishing WASM as Reference Spec & 32-bit Wrapping Arithmetic (P3 Audit Task)
- **Declared WASM Semantics as Spec**: Updated `AIPL_SPEC.md` specifying WASM as the reference specification.
- **Enforced 32-bit Wrapping Integers in VM**: Updated `src/vm.rs` so all arithmetic (`+`, `-`, `*`), bitwise, and shift ops apply 32-bit wrapping (`as i32`) and shift masking (`& 31`), and fixed division/mod bounds and zero checks.
- **Added Unsigned Opcodes**: Implemented `ShrU` (`shru`), `DivU` (`divu`), and `RemU` (`remu`) across `ast.rs`, `parser.rs`, `checker.rs`, `vm.rs`, `compiler/wasm.rs`, and `test_opcode_conformance.rs`.
- **Rejected Unsupported `i64` Scalars**: Modified `parse_type` in `src/parser.rs` to explicitly reject `i64` scalar type (`Err("i64 type is unsupported")`).
- **Added `tests/test_differential.rs`**: Created differential test runner comparing VM and Wasmtime execution across `examples/*.aipl`, `aipl_src/codegen.aipl`, and explicit edge cases (`+` overflow, arithmetic `shr`, `*` overflow, `/` truncating, `%` remainder, `shru`, `divu`, `remu`, `loop` end bound inclusive, and `while` with `set!`). All differential tests pass.


### Rust side (`src/`) — infrastructure fixes, not application logic
- **`mem.load8` / `mem.store8`** — new opcodes, full stack (`ast.rs`,
  `parser.rs`, `checker.rs`, `vm.rs`, `compiler/wasm.rs`). Replaces the
  fragile "4-byte store for a 1-byte value" convention.
- **Three independent WASM codegen bugs found and fixed** in
  `compiler/wasm.rs` (all three only surface with realistic, non-trivial
  control flow — trivial examples never hit them):
  1. `is_void_expr` had no `Expr::If` case, so nested `if`s were
     misclassified for stack-balance purposes.
  2. Statement-position values that nothing consumes weren't `drop`ped, and
     a `call` to a void-returning function was wrongly assumed to produce a
     value. Fixed by threading function-return-type info through codegen
     (`Ctx.fn_returns`) and inserting `Drop` where needed.
  3. **`loop`'s induction variable was never allocated a wasm local at all**
     (`collect_lets` didn't know about `Expr::Loop`'s `var` field), so the
     entire loop body was silently dropped and a value leaked onto the
     stack. Now errors loudly (`"loop variable '{}' has no local slot"`) if
     this class of bug ever recurs, instead of silently no-op-ing.
- **`(import name)` / `(import name as alias)`** — parser support
  (`parser.rs`) + a resolver (`src/resolver.rs`) that flattens imports into
  one qualified-name (`module_name.fn_name`) flat program before the
  existing checker/VM/wasm backend ever runs. Handles aliasing, transitive
  imports, diamond-dependency de-duplication (verified — was buggy, fixed),
  and circular-import detection. **Explicitly marked as temporary
  scaffolding** — see "The honest constraint" below.
- **`/compile` endpoint** added to the agent RPC server
  (`src/agent_api/server.rs`), alongside existing `/eval`/`/verify` — parses,
  type-checks, and compiles posted source to real WASM, base64-encoded (hand
  written, no new crate dependency), with CORS headers so `web/index.html`
  can call it directly.
- `web/index.html` — browser runtime interface.

### AIPL side (`aipl_src/compiler.aipl`) — the actual self-hosting work
- **Tokenizer is real**: `tokenize` + 9 helpers (`is_whitespace`, `is_delim`,
  `is_digit`, `write_token`, `is_arrow`, `is_true_kw`, `is_false_kw`,
  `is_int_literal`, `parse_int_literal`). Scans real source bytes; handles
  parens/brackets/colons/arrows/symbols/signed-int-literals/bools/
  strings/comments. Token record: 12 bytes `[kind, a, b]` at
  `tok_ptr + idx*12`.
- **Parser is real**: `parse_ast` + `parse_node` + `ast_alloc_node` +
  `tok_field`. A recursive-descent reader turning the token stream into a
  generic S-expression tree in memory — not yet a grammar-aware typed AST,
  see "Next steps". Node record: 16 bytes `[kind, a, b, next_sibling]`.
  Mutable state shared across recursive calls (token position, allocator
  cursor) lives in memory cells at a well-known offset from the buffer
  pointer, not in return values — AIPL functions only return one value. This
  pattern is documented inline in `compiler.aipl` and will be needed again.
- **Self-tests, not just shape checks**: `run_tokenizer_tests` (2 tests) and
  `run_parser_tests` (2 tests) build real byte strings in memory and assert
  exact token/node counts, kinds, and offsets — each was checked to actually
  *fail* when an assertion is deliberately broken (not vacuously true). This
  directly replaces the false-confidence problem in `tests/test_v2.rs`
  described in `LANGUAGE_GAPS.md` §2.

### A real language-design finding (not yet fixed, needs a decision)
`set!` has a real AIPL type (the variable's declared type) but its wasm
codegen never leaves a value on the stack. Two `if` branches that each
`set!` a *differently-typed* variable can't satisfy both the type checker
(same AIPL type required) and the wasm backend (same stack effect required)
using `set!`'s value alone. Worked around locally in `parse_node` by ending
both branches in an explicit real value; the real fix is probably making
`set!`'s AIPL type `void`, which is a breaking type-system change — see
`LANGUAGE_GAPS.md`'s update block for full detail. Don't make this change
casually; it needs its own pass across the checker and any code that relies
on `set!`'s current typing.

## The honest constraint behind `resolver.rs`

**AIPL has no file I/O opcode at all.** There is no way for AIPL code to
open or read a file today. That's why import resolution had to be written
in Rust — not a preference, a hard blocker. `src/resolver.rs` is explicitly
commented as temporary: once minimal WASI file primitives
(`path_open`/`fd_read`/`fd_close`) are wired into the wasm backend, this
~150-line Rust module should be deleted and rewritten as real AIPL calling
those primitives directly. Decided explicitly this session: keep the Rust
stopgap for now, prioritize `parse_ast`/codegen (pure in-memory AIPL, no I/O
needed) first. Revisit file I/O once the codegen work below is further
along.

## Decisions already made this session (don't re-litigate without cause)

- **Import syntax is qualified-by-default**: `(import name)` /
  `(import name as alias)`, calls as `module_name.fn_name`. Chosen
  specifically because the goal is code from many different, uncoordinated
  AI authors composing safely — bare/unqualified imports risk silent name
  collisions, which is exactly the kind of ambiguity this language is
  supposed to eliminate.
- **Cross-platform compile target is WASM+WASI** — confirmed, not up for
  debate; it's the only vendor-neutral standardized ABI with real I/O
  outside a browser.
- **"Fast as assembly" native target: WASM + external AOT compiler for now**
  (e.g. Cranelift via wasmtime), explicitly chosen over hand-rolling a
  native ELF/PE/Mach-O backend per architecture. This *defers* full
  sovereignty on the native path in exchange for reaching real performance
  sooner. `elf_emitter.aipl` is not the current priority as a result — don't
  invest further there without revisiting this decision first.
- **Rust work should be limited to primitives/infrastructure, not
  application logic.** Bug fixes to the existing bootstrap compiler and new
  opcodes are fine (they're "add a CPU instruction," not "do AIPL's job").
  Writing compiler *logic* (parsing, resolution, codegen policy) in Rust is
  what to avoid — that's what pushed the resolver decision above. Keep this
  distinction in mind for every future piece of work: ask "is this a
  primitive or is this logic?" before reaching for Rust.
- **Native multithreading is a real, reachable goal for the WASM target**
  (shared memory + atomics + host `thread_spawn` via wasi-threads — the
  language already has `atomic.lock/unlock/add/cas` in its grammar) but is
  ranked well behind finishing real codegen. The atomic ops are currently
  complete no-ops in both the VM and wasm backend, and shared memory isn't
  even turned on (`wasm.rs` hardcodes `shared: false`). Don't start this
  before `emit_wasm_binary` is real.

## Codegen (new)

`aipl_src/codegen.aipl` is a real, working code generator — pure AIPL, no
Rust changes needed for the codegen logic itself (one real bug it *found*,
in `wasm.rs`'s existing `loop` exit condition, was fixed separately, see
`LANGUAGE_GAPS.md` §2). It imports `compiler` for `tokenize`/`parse_ast`, then:
a keyword table + `classify_keyword` (turns a symbol's byte span into an
operator/form/type id — the generic tree has no idea `"if"` means anything
until this exists), a function-signature pass (`collect_functions`), a
per-function local-variable pass (`collect_locals_for_function`, including
loop induction variables), and the actual instruction emitter
(`compile_expr`/`compile_stmt`/`compile_function_body` and friends) covering
the same real subset `wasm.rs` supports: arithmetic/bitwise/comparison/
logical ops, `mem.load/store` 8/32/64, `let`/`set!`/`if`/`call`/`block`/
`while`/`loop`. Verified by compiling and running two real functions (`add`,
and a `compute` exercising `let`+`call`+`loop`) through `wasmtime` and
checking the actual returned numbers against hand-computed expected values,
not just "did it produce plausible-looking bytes."

**Hard-won lesson for whoever extends this next**: hand-typing deeply nested
`(if ... (if ... (if ...)))` chains in AIPL is extremely easy to get
paren-count wrong in a way that *doesn't* produce a parse error — a premature
closing paren just ends the `(module ...)` early and silently drops every
function defined after it, or leaves a function "open" so everything after it
becomes nested (and mis-scoped) inside it. Either way `aipl verify` reports
success because the *shorter/differently-nested* program is still valid on
its own. If a function you just added reports "not found" when invoked, this
is almost certainly why — check paren balance with a script, don't eyeball a
1900-character line. **Building nested/generated AIPL code as a small
S-expression builder in Python first (a form is a tuple, an atom is a string,
render() recurses) and rendering it to text is dramatically more reliable
than hand-typing once nesting gets more than 3-4 levels deep** — several
functions in `codegen.aipl` were built this way after hand-typed versions had
exactly this bug.

## Task P4: Spanned AST & Diagnostic Reporting (Completed)

- **Token Location Tracking**: `Token` converted to `struct Token { kind: TokenKind, line: u32, col: u32 }` tracking 1-based line/col positions during tokenization.
- **AST Span Information**: Attached `span: (u32, u32)` to `FnDef` and every `Expr` variant in `src/ast.rs`, with `Expr::span(&self) -> (u32, u32)`.
- **Unified Diagnostic Format**:
  - All `Parser` errors formatted as `"<line>:<col>: <message>"`.
  - All `TypeChecker` errors formatted as `"<line>:<col>: <message>"`.
  - All `Resolver` errors prefixed with `<path>: <message>`.
- **Lexical Rules & Edge Cases**:
  - Unterminated string literals error with `"L:C: Unterminated string literal"`.
  - Extra tokens after module end error with `"unexpected tokens after module end at L:C — check for an extra ')'"`.
  - Float literals strictly require a digit and `.`. Tokens like `inf` or `nan` parse as symbols/ops instead of floats.
- **Diagnostic Verification**: `tests/test_diagnostics.rs` asserts exact `L:C:` prefixes for missing `)`, unknown op, type mismatch in `let`, undefined variable, and extra `)` after module end.

## Next steps, in order

1. **General module assembly** — `codegen.aipl`'s verification hand-assembled
   the type/function/memory/export sections for the specific 1-2-function
   test cases. A real `emit_module`-style function that does this for *any*
   number of functions with varying signatures (including grouping local
   declarations by type, which the verification also hardcoded by hand) is
   the concrete next piece.
2. **Wire it up**: `compiler.aipl`'s public `compile_to_target`/`compile_aipl`
   still point at the old hardcoded stub. Once module assembly (step 1)
   exists, make those delegate to `codegen.aipl` (via `(import codegen)`) so
   `aipl compile` and the self-hosted path are the same real path, not two
   diverging ones.
3. Rewrite `tests/test_v2.rs`'s self-hosting tests to exercise varied,
   non-trivial input (they currently only check output starts with the WASM
   magic bytes and is `>20` bytes — would pass on garbage input) — do this
   once steps 1-2 land, matching what the real pipeline now produces.
4. Then: WASI file I/O primitives, so `resolver.rs` can finally be deleted
   and rewritten as AIPL (see "the honest constraint" above).
5. Longer-term backlog (not urgent, don't start yet): the `set!`-typing
   design tension above; the rest of `LANGUAGE_GAPS.md` §6 (structs,
   generics, closures, real arrays/strings-in-wasm, a standard library);
   native ELF codegen if the AOT-compiler decision ever gets revisited;
   threading once codegen is solid.

## How to re-verify everything above in a fresh session

```bash
# Build + full regression suite (should be 15/15 passing)
wsl bash -lc "cd /mnt/c/Users/MattWarpinski/open_source_project/AIPL && cargo test 2>&1 | tail -30"

# Run the AIPL-native self-tests through the real VM
wsl bash -lc "cd /mnt/c/Users/MattWarpinski/open_source_project/AIPL && cargo run --bin aipl -- eval aipl_src/compiler.aipl --func run_tokenizer_tests"
wsl bash -lc "cd /mnt/c/Users/MattWarpinski/open_source_project/AIPL && cargo run --bin aipl -- eval aipl_src/compiler.aipl --func run_parser_tests"
# Both should print [AIPL Result]: Int(2)

# Compile to real WASM and verify through wasmtime too
wsl bash -lc "cd /mnt/c/Users/MattWarpinski/open_source_project/AIPL && cargo run --bin aipl -- compile aipl_src/compiler.aipl -o /mnt/c/Users/MattWarpinski/open_source_project/AIPL/aipl_src/compiler_check.wasm"
"C:\Users\MattWarpinski\AppData\Local\Programs\Python\Python312\python.exe" -c "
from wasmtime import Store, Module, Linker
store = Store(); module = Module.from_file(store.engine, 'aipl_src/compiler_check.wasm')
instance = Linker(store.engine).instantiate(store, module)
exports = instance.exports(store)
print('run_tokenizer_tests ->', exports['run_tokenizer_tests'](store))
print('run_parser_tests ->', exports['run_parser_tests'](store))
"
# delete aipl_src/compiler_check.wasm afterward - it's a scratch verification artifact, not a repo deliverable
```

## Task: `i64` as a first-class type (Completed 2026-09-18)

P3 left `i64` rejected in `parse_type` ("document that i64 is unsupported"). It is now a real type in all four stages, with wasm as the semantic reference:

- **Literals**: `42i64` / `-7i64` (suffix is part of the token) -> `TokenKind::Int64Lit` -> `Literal::Int64` -> `Type::I64`. Plain `42` stays `i32`; there is no implicit widening.
- **Conversions**: three new opcodes, `i64.extend_s`, `i64.extend_u`, `i32.wrap`, mapping to `i64.extend_i32_s`, `i64.extend_i32_u`, `i32.wrap_i64`. Checker enforces `i32 -> i64` / `i64 -> i32` argument types.
- **VM**: new `Value::Int64(i64)`. Every arithmetic, bitwise, shift, division, and comparison op has an `(Int64, Int64)` arm with 64-bit wrapping (shift counts masked to 6 bits, `/` errors on zero and `MIN/-1`, `%` of `MIN/-1` is 0). `mem.load64` returns `Int64`; `mem.store64` requires it.
- **Wasm**: `Ctx` now carries `local_types`; a new `expr_type` computes the static type of any expression, and `arith_instruction` / `compare_instruction` select `i32.*` / `i64.*` / `f32.*` / `f64.*` variants. `if` block result types come from the branch type instead of being hard-coded `I32` (this was the "`if` with `i64`/`f64` branches gets `BlockType::Result(I32)` — invalid" bug from the audit's section 2). As a side effect **`f64` arithmetic and comparisons now compile to valid wasm**; previously `(+ 1.0 2.0)` emitted `i32.add` and failed validation.
- **Tests**: `tests/test_i64.rs` (12 tests) runs every case in the VM and validates the wasm with `wasmparser`: literals, 64-bit wrap points, div/rem/shift edge cases, comparisons, `if` with `i64` branches, all three conversions, `mem.store64`/`mem.load64` round trip, `i64` params and calls, an `i64` accumulator over an `i32` loop, checker rejections for mixed widths, and the `f64` regression. `tests/test_opcode_conformance.rs` covers the three new opcodes, and its `mem.load64`/`mem.store64` programs (which declare `-> i64`) now actually execute instead of being skipped at parse time.
- **Not changed**: loop bounds, addresses, sizes, fds, and thread handles stay `i32`. `codegen.aipl` (the self-hosted backend) does not know about `i64` yet. `f32` literals still do not exist (float literals are `f64`).

## Task P3 (testing half): VM-vs-wasm differential testing (Completed 2026-09-18)

- **`wasmtime` dev-dependency** added (48.x). Tests compile AIPL to wasm with `WasmCompiler`, instantiate the bytes in wasmtime, and call the export directly. No imports are needed because the backend emits none.
- **`tests/test_differential.rs`**: a `differential(module, wasm, fn, args)` helper runs the same function in `VM` and wasmtime and asserts identical results, or that both fail. `bool` results are normalised to `Int(0|1)` (their wasm shape). A VM contract failure (`req`/`ens`) is the one accepted asymmetry, because the wasm backend does not emit contracts.
  - Explicit i32 cases: `(+ 2147483647 1)`, `(- -2147483648 1)`, `(shr -8 1)`, `(shru -8 1)`, `(* 65536 65536)`, `(/ -7 2)`, `(% -7 2)`, `(divu -1 2)`, `(remu -1 2)`, `(shl 1 33)`, `(shr -2147483648 32)`, `(% -2147483648 -1)`; division by zero and `MIN / -1` fail in both.
  - Control flow: inclusive `loop` end bound (0..5 sums to 15), `loop` with step 3 and negative start, `while` mutating a `let` via `set!`, nested `if`/`block` values, recursion (`fib 20`), `mem.alloc`/`mem.store*`/`mem.load*` round trip including bump-allocator spacing.
  - i64 cases: wrap at 2^63, no wrap at 2^31, `i32.wrap`/`i64.extend_s`/`i64.extend_u`, 6-bit shift mask, unsigned div/rem, division edge cases, `if` with i64 branches, `mem.store64`/`mem.load64` mixed with `mem.load32`.
  - f64: `(/ (+ 1.5 2.25) 0.5)` and a float comparison (regression for the old `i32.add`-on-floats codegen bug).
  - **Whole programs**: every `examples/*.aipl` is resolved, checked, compiled, and every function with an `i32`/`i64`/`bool` return and all-`i32` params is run in both backends over a fixed set of argument tuples (`0, 1, 7, 50, 100, -1, i32::MAX` in every position plus two mixed tuples). Files the wasm backend rejects are skipped with a printed reason; `hello_browser.aipl` is pinned as known-stale (uses removed `dom.*`/`web.alert` ops) and the test fails if it ever starts parsing without being removed from that list. The test asserts at least 4 files and 40 calls were actually compared so it cannot go vacuous.
  - **`aipl_src/codegen.aipl`**: `test_signatures_and_locals` is zero-arg `i32`, but the module also contains `test_compile_add`/`test_compile_compute`, which use `fs.open`/`fs.write`/`fs.close`, so the wasm backend rejects the whole module. The test pins that exact reason and automatically switches to a real comparison the day the module compiles.
- **Divergence fixed in the VM**: `Expr::Loop` had an overflow guard (`if st_val > 0 && curr < s_val { break; }`) that wasm does not have. Removed; a loop whose counter wraps past `i32::MAX` now never terminates in either backend, matching the reference.
- **Spec**: "wasm semantics are the spec" recorded directly under the title of `AIPL_SPEC.md`.

## Task P5: One memory layout, one allocator, no hardcoded table addresses (Completed 2026-09-18)

- **Fixed layout** (AIPL_SPEC.md, "Memory layout"): bytes 0-1023 are the runtime block, the heap begins at 1024. Cell 0 is the heap cursor, cell 4 the compile-error flag, cells 16/20/24/28 belong to codegen (keyword table ptr, function table ptr, default locals table ptr, running locals count). Everything else below 1024 is reserved and must not be written by programs.
- **One allocator.** `SharedMemory.heap_ptr` is gone from `src/vm.rs`; `mem.alloc` reads and writes the i32 at address 0, which `VM::new` initialises to 1024. The wasm backend does the same with `i32.load`/`i32.store` at address 0 and initialises the word through an active **data segment** (the mutable global is gone). Self-hosted AIPL (`memory.aipl`) was already keeping its cursor at address 0 but with a private start of 8192; it is now a thin wrapper over `mem.alloc`. Three cursors became one.
- **Memory size.** Both backends start at 16 pages (1 MiB) and cap at 100. The wasm minimum was 1 page before; raising it to match the VM keeps `mem.grow` results identical across backends and removes the last asymmetry in memory limits.
- **`mem.grow`** (`OpCode::MemGrow`): grows by N pages and returns the old size in pages, or -1 past the 100-page cap. VM resizes the `Vec`; wasm emits `memory.grow`. Covered by the conformance test. (The prompt named it `memory.grow`; `mem.grow` follows the `mem.*` family every other memory op uses.)
- **`codegen.aipl`**: every table now comes from `(mem.alloc N)` in `codegen_init` (idempotent, called by `init_keywords`), reached via `kw`/`fn_table`/`locals_table` accessors over cells 16/20/24; the locals count lives in cell 28 and the error flag in cell 4. All 171 `(+ 20000 N)` keyword sites, the 43 `200xx` literal pointers in `classify_keyword`, and every test buffer (`500`, `9000`, `12000`, `30000`, `41000`, `42000`, `60000`, `61000`, `70000`, `400`) were replaced. `grep -o '\b[0-9]\{4,\}\b' aipl_src/codegen.aipl` now returns only allocation sizes (3072, 3584, 4096, 8192, 16384). A `run_codegen_tests` group runner (returns passes, 3 = all) is wired into `test_suite.aipl`.
- **Other modules moved off literal addresses**: `compiler.aipl` self-test buffers (8000-9900), `thread_sync.aipl` (counter at 700 and name at 800; the worker now receives the counter pointer as its thread argument), `file_io.aipl` (path/payload/read buffers at 400/500/600), and the `tests/test_v2.rs` mutex test (96/100).
- **A latent hang this exposed**: the conformance test's `(atomic.lock 0)` spun forever once address 0 held the cursor (1024, never zero). All memory/atomic conformance programs now operate on `(mem.alloc N)` words.
- **New `tests/test_selfhost.rs`**: runs `test_compile_add` and `test_compile_compute` from `codegen.aipl` in the VM, validates the emitted bytes with wasmparser, executes them in wasmtime, and asserts `add(2,3)=5`, `add(MAX,1)=MIN`, `compute(1)=51`, `compute(10)=60`, `compute(-50)=0`. This is the executable form of the prompt's `aipl test aipl_src/codegen.aipl --func test_compile_compute` check (that subcommand treats a non-zero return as a failure count, and the self-test returns a byte length).

## Follow-up: memory layout is enforced, not just documented (2026-09-18)

P5 moved the heap cursor into address 0 and immediately exposed a hang: the conformance test's `(atomic.lock 0)` spun forever on a word that now holds 1024. Patching that one test was not a fix, so the layout is now enforced in two places:

- **Checker (`src/checker.rs`, `check_literal_address`)**: every `mem.*` / `atomic.*` op whose address argument is an integer literal is checked against the layout. Stores or locks on bytes 0-3 (the cursor), any access to bytes 64-1023 (reserved), and misaligned cells in 4-63 are `L:C:`-prefixed errors before either backend runs. Reading the cursor and using aligned runtime cells stay legal (memory.aipl, codegen.aipl). Covered by five new tests in `tests/test_diagnostics.rs`.
- **VM (`src/vm.rs`)**: `atomic.lock` now fails with `... not a lock state (0 = free, 1 = held) ...` when the word holds anything other than 0 or 1, instead of spinning; `atomic.unlock` fails when the word is not 1. A lock word is only ever 0/1 by construction, so this has no false positives and catches the computed-address case the checker cannot see. The wasm backend rejects atomics anyway, so no backend divergence is introduced.
- **`tests/test_memory_layout.rs`** (7 tests): fresh VM and fresh wasm instance both have 1024 at address 0 (data segment verified by reading wasmtime memory) and both bump it identically; locking a word holding 1024 errors; locking address 0 through a computed `(- p p)` errors; unlocking a free word errors; a real lock round trip still works; `mem.grow` returns 16 then refuses past 100 pages.
- Two existing tests that still used address 0 (`test_i64` store64/load32 case, conformance `atomic.unlock`) were moved to `mem.alloc` words; the unlock program now locks first, since unlocking a free word is an error.

## Follow-up: computed writes into the reserved block are caught in both backends (2026-09-18)

Closes the last enforcement gap from the memory-layout work. The checker only sees literal addresses; a store whose address is computed at runtime could still land in bytes 0-3 (heap cursor) or 64-1023 (reserved) unnoticed.

- **VM** (`src/vm.rs`, `check_write_address`): every `mem.store8/32/64` and every `atomic.add/cas/lock/unlock` checks its address before writing and fails with `<op> at address N: bytes 0-3 are the heap cursor ...` or `... bytes 64-1023 are the reserved runtime block ...`.
- **Wasm backend** (`src/compiler/wasm.rs`, `emit_write_address_check`): before every `i32.store8/i32.store/i64.store` the backend emits `local.tee s; (s <u 4) | ((s - 64) <u 960); if unreachable end`, using one extra `i32` local per function. Same addresses, same outcome (trap), so the differential test treats VM error + wasm trap as agreement and no VM-only semantics were introduced.
- Reads are deliberately unchecked (the block is zero; reading it is harmless), and `mem.alloc`'s own update of the cursor bypasses the check since it is emitted directly.
- **Tests** (`tests/test_memory_layout.rs`, now 13): computed store to 512 fails in the VM and traps in wasmtime; computed store to address 0 likewise; store64 at 1023 fails while 1024 succeeds in both; computed writes to runtime cell 16 and to heap words succeed in both; computed reads from the block succeed; all four atomics fail on a computed reserved address.
- Known remaining asymmetry: the self-hosted `codegen.aipl` does not emit this check in the wasm it produces yet.

## Follow-up: the self-hosted compiler emits the memory-layout store guard too (2026-09-18)

Closes the codegen asymmetry left by the previous follow-up.

- **`aipl_src/codegen.aipl`**: new `emit_store_guard [ptr scratch_idx]` writes the exact byte sequence the Rust backend's `emit_write_address_check` produces (`local.tee s; local.get s; i32.const 4; i32.lt_u; local.get s; i32.const 64; i32.sub; i32.const 960; i32.lt_u; i32.or; if; unreachable; end`). `compile_op` calls it between the address and the value for `mem.store8/32/64` (op ids 12/13/14). The scratch local's index is the function's total local count, so every function body now declares one extra `i32` local; the three hand-assembled test harnesses were updated (`add`: 0 -> 1 local, `compute`: 0/3 -> 1/4) with their code-section lengths adjusted, and their memory sections raised from 1 to 16 pages to match the Rust backend.
- **New self-test `test_compile_store`** compiles `(fn add [a:i32 b:i32] -> i32 (mem.store32 a b) (mem.load32 a))` to `aipl_src/_codegen_out3.wasm`; `run_codegen_tests` now reports 4 and `test_suite.aipl` expects 4.
- **`tests/test_selfhost.rs`** (now 4 tests): the self-hosted `add` traps in wasmtime for addresses 0, 3, 64, 512, 1023 and succeeds for 16, 1024, 65536; and its function body is **byte-for-byte identical** to the Rust backend's output for the same program (both bodies extracted with wasmparser and compared), so the two backends cannot drift apart on this silently. The helper that runs self-tests is serialised with a mutex because they write fixed output paths.

## Task P6: WASI imports and data segments - compiled AIPL that does I/O (Completed 2026-09-18)

- **Imports** (`src/compiler/wasm.rs`): an `ImportSection` from `wasi_snapshot_preview1` with `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, and `path_unlink_file` (the sixth is needed by `fs.delete`, which the prompt did not list). Only the functions a module actually uses are imported, in a fixed order, so a module without I/O has no import section and keeps instantiating with no imports; user function indices are offset by the import count and import types come first in the type section.
- **Lowerings**: `sys.print` -> `fd_write` to fd 1 (one call per iovec: string bytes, then an interned `"\n"`; wasmtime honours only the first iovec of a call, which cost one debugging round). `fs.open` -> `path_open` on the preopened dir (fd 3) with `oflags = CREAT|TRUNC` and rights `FD_READ|FD_WRITE` when `flags != 0`, else rights `FD_READ`; `fs.read`/`fs.write` -> `fd_read`/`fd_write` with one iovec; `fs.close` -> `fd_close`; `fs.delete` -> `path_unlink_file`; `sys.exit` -> `proc_exit`. Every errno collapses to `-1`, matching the VM. Operands are evaluated left to right and unloaded into two per-function I/O scratch locals (added only to functions that do I/O), so nested I/O expressions are safe. Runtime cells 64-87 hold the iovecs and out-parameters (documented in AIPL_SPEC.md 7.9).
- **Strings**: every `Literal::Str` is interned once into a data segment at 512-1023 as `[len u32 LE][bytes]`; the literal compiles to `i32.const <address of bytes>`; a compile error names the area if a module needs more than 512 bytes. New op `(str.len s)` (`OpCode::StrLen`, checker: `str -> i32`, VM: Rust length, wasm: `i32.load (s-4)`). `eq`/`neq` on `str` compare interned pointers, so equal literals compare equal in both backends. `(+ str str)` stays VM-only.
- **Checker**: `fs.*` arguments must be `i32` (a `str` path would have worked in wasm and failed in the VM); `sys.exit` takes one `i32`; `sys.print` type-checks its arguments.
- **VM**: `sys.exit` no longer reports "not supported"; it returns `sys.exit(N) requested` instead of terminating the host process.
- **Mixed-void `if` fix** (surfaced by compiling `codegen.aipl`): `(if c (set! x v) 0)` type-checks (set! has the variable's type) but `set!` leaves nothing on the wasm stack, and the backend chose the block type from the `then` branch alone, producing invalid wasm. The backend now gives such an `if` a result type and tops up the value-less branch with the assigned variable (or 0), so both backends agree. P7 will make `set!`/`let` void and retire this.
- **Tests**: `tests/test_wasi.rs` (8 tests, `wasmtime-wasi` dev-dependency): `file_io.aipl`'s `run_file_io_tests` returns 1 under WASI with a preopened temp dir and in the VM, leaving no file behind; `sys.print` output is exactly one line per argument; interned strings report lengths and compare by identity; data-area overflow is a compile error; opening a missing file returns -1 in both; a write-then-read round trip agrees byte for byte and the wasm file is visible on the host; `sys.exit 7` is `I32Exit(7)` from wasmtime and `sys.exit(7) requested` from the VM; I/O-free modules have no imports. The differential harness now links WASI, and the previously pinned codegen test became a real comparison: **the self-hosted compiler's `test_signatures_and_locals` (tokenizer + parser + signature and locals collection, ~80 functions) runs under wasmtime and returns the same result as the VM.** Conformance covers `str.len`. Total: 91 Rust tests, AIPL suite green.

## Follow-up: an AIPL program that does I/O end to end (2026-09-18)

P6 made the *toolchain* able to compile I/O; nothing in `.aipl` demonstrated it. Now it does, and two small language additions made the demonstration honest instead of a wall of `mem.store8`:

- **`(str.ptr s)`** (`OpCode::StrPtr`, `str -> i32`): the address of a string's bytes, for the pointer-taking `fs.*` ops. Identity in wasm (a `str` already is that pointer); the VM materialises the string into the heap as `[len][bytes]` and returns the address of the bytes. With `str.len` this closes the "no str -> (ptr, len) conversion" gap.
- **fds 1 and 2 are stdout/stderr in the VM**, as under WASI, so `(fs.write 1 buf n)` prints formatted bytes in both backends and AIPL code can print numbers without a built-in formatter.
- **String escape sequences** in the tokenizer: `\n \t \r \0 \ \"`; an unknown escape is an `L:C:`-prefixed error.
- **`examples/word_count.aipl`** reads `input.txt`, counts lines, words, and bytes, prints them (`print_uint` formats digits by hand into a `mem.alloc` buffer), writes its error to fd 2, and returns the line count. `examples/input.txt` is a sample (4 lines, 15 words, 81 bytes). Runs with `aipl eval` from `examples/` and with `wasmtime run --dir=.` after `aipl compile`.
- **Tests** (`tests/test_wasi.rs`, now 13): the example gives `lines: 4 / words: 7 / bytes: 34` and returns 4 in both backends for a generated input, reports a missing file on stderr with -1 in both, reproduces the documented counts for the shipped sample, `str.ptr`/escapes/str variables agree across backends, and `fs.write` to fd 1 reaches stdout in both. The differential examples loop skips I/O examples (WASI imports) with a printed reason since they are compared under a preopened directory here instead. Conformance covers `str.ptr`. Total: 96 Rust tests.
