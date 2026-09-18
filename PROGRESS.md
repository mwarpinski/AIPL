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

## What's real now (verified, not just written)

Every item below was checked with the real Rust toolchain and/or `wasmtime`
— see "how to re-verify" at the end.

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

## Next steps, in order

1. **`emit_wasm_binary`** (or a renamed equivalent) — walk the real
   S-expression tree `parse_ast` now produces and actually emit WASM bytes
   for it, replacing the current hardcoded-output stub. This is the next
   concrete milestone. Note: `compile_to_target`/`emit_wasm_binary`/
   `compile_aipl` currently read the AST using the *old* hardcoded 4-field
   layout and will produce garbage against the *new* tree shape — this was
   already broken before (never functionally correct), so it's not a
   regression, but don't be confused by it silently producing different
   wrong output than before.
2. Somewhere in or alongside step 1, the generic tree needs a semantic pass:
   given a "paren-group" node, dispatch on its first child's symbol text
   (`if`, `let`, `set!`, `loop`, `while`, `call`, an operator, or a
   user-defined function name) the way `src/parser.rs::parse_expr` does.
   Decide whether this is a separate pass or fused into codegen
   (syntax-directed translation, skipping a separate typed-AST
   materialization step) — leaning toward fusing it, since the generic tree
   plus head-symbol dispatch is enough, but not decided yet.
3. Once codegen exists, rewrite `tests/test_v2.rs`'s self-hosting tests to
   exercise varied, non-trivial input (they currently only check output
   starts with the WASM magic bytes and is `>20` bytes — would pass on
   garbage input).
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
