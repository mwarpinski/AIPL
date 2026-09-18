# AIPL Language Gap Analysis

Companion document to [AIPL_SPEC.md](AIPL_SPEC.md), [PROMPT_GUIDE_FOR_AIS.md](PROMPT_GUIDE_FOR_AIS.md),
and [PROGRESS.md](PROGRESS.md) (session handoff / how-to-verify notes). Where
those describe what AIPL is supposed to do, this describes what it actually
does *right now*, kept current as work lands rather than left to accumulate
contradictory update notes. If you're an agent (or a human) about to "fix" or
"add" something below, check the file/line references first — several things
this document used to call missing have since been built, by more than one
contributor to this repo.

Verification method: every claim below was checked against the real
implementation, not inferred from reading source or trusting a self-test's
own "SUCCESS" return value. A real Rust toolchain and a real WASM runtime
(`wasmtime`, via Python bindings) are both available in this environment now
— see [PROGRESS.md](PROGRESS.md) for exact setup/verification commands.
Anything still marked as a gap here was re-checked against the current code,
not left over from an earlier pass.

---

## 1. Committed `.wasm` binaries drift from source

Three committed `.wasm` files still fail to load in a real, spec-compliant
WASM runtime — re-verified just now, not a stale finding:

| File | Result |
|---|---|
| `aipl_compiler.wasm` | Loads. Exports `tokenize`, `parse_ast`, `emit_wasm_binary`, `compile_aipl`, etc. — but see §2, this binary predates the real tokenizer/parser and was never rebuilt, so it still behaves like the old stub. |
| `aipl_db.wasm` | **FAILS.** `type mismatch: expected i32 but nothing on stack` at byte offset 327. |
| `aipl_sovereign_toolchain.wasm` | **FAILS.** `type mismatch: values remaining on stack at end of block` at offset 1107. |
| `examples/aipl_database/aisql_engine.wasm` | **FAILS.** `type mismatch: expected i32 but nothing on stack` at offset 232. |

(The HN clone demo and its `.wasm` files that used to be discussed here —
`hn_full_engine.wasm`, `hn_cli.wasm`, `examples/hn_clone/*` — were deleted
from the repo in a later pass ("early testing to make a hacker news clone and
it was garbage"). That finding no longer applies to anything that exists.)

The general lesson stands and is worth keeping: committing compiled binaries
instead of building from source means artifacts silently drift from the code
that supposedly produced them, and nothing catches it until someone actually
tries to load the binary. `aipl_compiler.wasm` at the repo root is a live
example right now — rebuild it from current `aipl_src/compiler.aipl` before
trusting it for anything.

## 2. Self-hosted compiler status: tokenizer, parser, AND core codegen are real

> **Update:** `aipl_src/codegen.aipl` (new file) now compiles real AIPL source
> — tokenized and parsed by `compiler.aipl`, then walked and emitted as real
> WASM instructions entirely in AIPL — for the same opcode/construct subset
> `src/compiler/wasm.rs` genuinely supports: arithmetic/bitwise/comparison/
> logical ops, `mem.load/store` at 8/32/64-bit, `let`/`set!`/`if`/`call`/
> `block`/`while`/`loop`. Verified with two real, non-trivial programs
> compiled end-to-end and independently checked with `wasmtime`: a 2-param
> `add` function, and a `compute` function exercising `let`, a cross-function
> `call`, and a `loop` — both produced valid WASM that returns the
> mathematically correct result for multiple real inputs, not just a
> structurally-plausible byte count. This is genuinely the hardest part of
> self-hosting the compiler.
>
> Building this **found a real, independent bug in the existing Rust
> `wasm.rs`** (not something introduced by the AIPL codegen mirroring it):
> `loop`'s exit condition used `i32.ge_s` (exclusive of `end`), while `vm.rs`'s
> interpreter runs `while curr <= end` (inclusive) — a WASM-compiled loop ran
> one fewer iteration than the same source run through the VM, silently,
> forever, until this AIPL implementation was checked against hand-computed
> expected output and disagreed. Fixed in `wasm.rs` to `i32.gt_s` (exit only
> once the counter exceeds `end`); the AIPL codegen was written to match.
>
> **Not yet done**: general module assembly for an arbitrary number of
> functions (the verification above hand-assembled the type/function/export
> sections for the specific 1-2-function test cases — `emit_module`-style
> code that does this for any real program is the next piece), and wiring the
> new codegen into `compiler.aipl`'s public `compile_to_target`/`compile_aipl`
> entry points (right now `codegen.aipl` is a separate module you call
> directly; those two still point at the old stub). Once both land, `aipl
> compile` and the self-hosted compiler converge into one real path.

This used to say the whole compiler was a stub that ignored its input. That's
no longer accurate for two of its three stages:

- **`tokenize`** (in `aipl_src/compiler.aipl`) is real: it scans actual source
  bytes and correctly handles parens/brackets/colons/arrows/symbols/signed-int
  literals/bools/strings/comments, verified through both the VM and real
  compiled WASM with content-assertion tests (`run_tokenizer_tests`).
- **`parse_ast`** is real: a recursive-descent reader (`parse_node`) that
  turns the token stream into a generic S-expression tree in memory (atoms +
  parenthesized/bracketed groups, 16 bytes/node) — not yet a grammar-aware
  typed AST (see the next section), but genuinely reads its input, verified
  the same way (`run_parser_tests`).

**Codegen is still being wired to real ASTs; stubbed functions removed**: `emit_wasm_binary` (which read AST fields using the old 4-field layout) and `compile_to_target`'s ELF branch were removed from `aipl_src/compiler.aipl`, and `compile_to_target` now returns `-1` with a comment `;; not yet wired to codegen.aipl`. Real WASM codegen lives in `aipl_src/codegen.aipl`.

### Quarantined Fabrications (`attic/`)
The following 7 fabricated `.aipl` modules were moved into a new `attic/` directory with `attic/README.md` documenting their fake return values:
- `sovereign_toolchain.aipl` (`fs_open` returned 10/11, `fs_read` returned count, `thread_spawn_sync` returned 101, `tokenize` counted parens only)
- `pipeline.aipl` (`pipeline_tokenize` returned 4 on zero tokens)
- `elf_emitter.aipl`
- `optimizer.aipl`
- `diagnostics.aipl`
- `aipl_test.aipl`
- `aipl_db.aipl`

Associated fake test runners (`src/bin/aipl_test_runner.rs` and `src/bin/aisql_runner.rs`) and their `Cargo.toml` `[[bin]]` entries were deleted. All 7 tests certifying these fabricated modules were removed from `tests/test_v2.rs` (`test_self_hosted_wasm_emitter`, `test_sovereign_aipl_diagnostics`, `test_sovereign_aipl_test_runner`, `test_dual_target_native_elf_emitter`, `test_heavy_optimizer_constant_folding`, `test_sovereign_wasm_roundtrip_execution`, `test_e2e_sovereign_pipeline_bootstrap`).

### OpCode Conformance & Fallback Elimination (P2 Audit Task)
All four silent wildcard `_ =>` fallbacks were eliminated:
1. `src/vm.rs` `eval_op` `_ => Ok(Value::Int(0))` replaced with explicit match arms and error reporting.
2. `src/compiler/wasm.rs` `compile_expr` `Op` and `Expr` wildcards `_ => Nop` replaced with explicit arms returning `Err(...)` for unsupported operations.
3. `src/checker.rs` `infer_expr_type` `_ => Ok(Type::I32)` replaced with explicit type inference arms.
4. `src/compiler/wasm.rs` `aipl_to_wasm_type` `_ => ValType::I32` replaced with explicit `Type` matching.

Additionally:
- `OpCode::Mod` implemented in VM (`%` with div-by-zero check) and WASM (`I32RemS`).
- `OpCode::MemAlloc` implemented in WASM as a bump allocator using Global 0 (`GlobalSection` initialized to 1024).
- `OpCode::SysPrint` in WASM configured to return explicit `Err`.
- 7 unused OpCodes (`VecDot`, `MatMul`, `DomElem`, `DomMount`, `DomAppend`, `DomOnEvent`, `WebAlert`) removed from `ast.rs`, `parser.rs`, `checker.rs`, and `AIPL_SPEC.md`.
- `tests/test_opcode_conformance.rs` added using `wasmparser` validation to enforce that every OpCode variant either (a) succeeds in VM + compiles/validates in WASM, or (b) returns an explicit `Err`.

### WASM Reference Spec & 32-Bit Wrapping Arithmetic (P3 Audit Task)
WebAssembly semantics are now officially established as the reference specification in `AIPL_SPEC.md`. Integer arithmetic in both VM and WASM backends is defined as wrapping 32-bit:
- `src/vm.rs` enforced wrapping `as i32` for `Add`, `Sub`, `Mul`, `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr`, `ShrU`, `Div`, `DivU`, `Mod`, `RemU`. Shift amounts are masked with `& 31`.
- Added `OpCode::ShrU` (`shru`), `OpCode::DivU` (`divu`), `OpCode::RemU` (`remu`) for unsigned operations across parser, checker, VM, WASM codegen, and conformance tests.
- Scalar `i64` type is explicitly rejected during parsing (`Err("i64 type is unsupported")`).
- Differential test suite `tests/test_differential.rs` added, comparing VM vs. Wasmtime execution for edge cases and example programs.



## 3. Byte-granularity memory ops — RESOLVED

Previously: every byte-buffer-writer relied on `mem.store32` at consecutive
offsets, which happened to produce correct output by an accident of
little-endian byte ordering, but had no real single-byte primitive and no
guard against a stored value silently corrupting neighboring bytes above 255.

**Fixed**: `mem.load8`/`mem.store8` now exist end-to-end (parser, checker, VM,
WASM backend). The real tokenizer and parser in `compiler.aipl`, and the new
`file_io.aipl`, use them directly. The old 4-byte-store convention still
appears in not-yet-rewritten files (`wasm_emitter.aipl`, the stub parts of
`compiler.aipl`'s codegen, `elf_emitter.aipl`) — not wrong, just worth
migrating to the real primitive when those files are next touched.

## 4. Type/opcode coverage matrix (parser → checker → VM → WASM backend)

Cross-referencing `src/ast.rs`'s `OpCode` enum (50 variants — 44 original +
6 added since) against every place it's consumed:

| OpCode | Parsed | Type-checked | VM (`vm.rs`) | WASM (`wasm.rs`) |
|---|---|---|---|---|
| Add, Sub, Mul, Div | Yes | Yes | Yes | Yes |
| **Mod** (`%`) | Yes | Yes | **No** (falls to `Int(0)`) | **No** (falls to `Nop`) |
| BitXor, Shl, Shr, BitAnd, BitOr | Yes | Yes | Yes | Yes |
| **MemLoad8/MemStore8** | Yes | Yes | Yes | Yes |
| MemLoad32/64, MemStore32/64 | Yes | Yes | Yes | Yes |
| **MemLoadF32/F64, MemStoreF32/F64** | Yes | Yes | **No** | **No** |
| MemAlloc | Yes | Yes | Yes (bump allocator, never frees) | **No** |
| MemFree | Yes | Yes | No-op (documented as no-op) | **No** |
| AtomicAdd | Yes | Yes | Yes (genuinely atomic — see §6) | **No** |
| AtomicCas | Yes | Yes | **Yes** (real compare-and-swap) | **No** |
| AtomicLock/Unlock | Yes | Yes | **Yes** (real spinlock on shared memory — see §6, no longer a fake side-table) | **No** (falls to `Nop`) |
| Eq, Neq, Lt, Lte, Gt, Gte | Yes | Yes | Yes | Yes |
| And, Or | Yes | Yes | Yes (**does not short-circuit** — see §5) | Yes (compiles to bitwise `i32.and`/`i32.or`, also non-short-circuiting, and wrong if operands aren't exactly 0/1) |
| Not | Yes | Yes | Yes | Yes |
| **VecDot, MatMul** | Yes | Yes (fixed return types) | **No** | **No** |
| **ArrGet, ArrSet** | Yes | **No** (falls through to a default `Type::I32`, i.e. untyped) | **No** | **No** |
| **DomElem, DomMount, DomAppend, DomOnEvent, WebAlert** | Yes | Yes | **No** | **No** (silent `Nop`) |
| SysPrint | Yes | Yes | Yes | **No** (silent `Nop` — a compiled program can never print) |
| **SysTime, SysExit** | Yes | Yes | **No** | **No** |
| **FsOpen/FsRead/FsWrite/FsClose** | Yes | Yes | **Yes** (real `std::fs` I/O) | **No** — explicit compile error, not a silent no-op (needs WASI) |
| **ThreadSpawn/ThreadJoin** | Yes | Yes | **Yes** (real `std::thread` OS threads) | **No** — explicit compile error, not a silent no-op (needs shared memory + wasi-threads) |

14 of 50 opcodes still silently return `0`/no-op in the VM instead of running
or erroring (down from 17 of 44 — `AtomicCas`/`AtomicLock`/`AtomicUnlock`
moved from the fake column to the real one, and the 6 new ops added real).
The remaining silent-no-op set (`Mod`, float memory ops, `MemAlloc`/`MemFree`,
`VecDot`/`MatMul`, `ArrGet`/`ArrSet`, DOM ops, `SysTime`/`SysExit`) means any
AIPL program using those still executes without any error message and
produces meaningless results in the VM — worse than a crash, because nothing
tells the author anything went wrong. The WASM backend's silent-`Nop` set is
similar, except the 6 new ops explicitly refuse to compile rather than
joining it — that pattern (loud error over silent no-op) is the right one and
worth applying to the rest of this list eventually.

Type-level gaps in `src/parser.rs::parse_type` are unchanged: only
`i32/i64/f32/f64/bool/str/void/arr/vec` are parseable, despite `Type::Ptr`,
`Type::Fn`, and `Type::ResultType` existing in the AST — a source file using
`(ptr i32)` or `(fn (i32) -> i32)` in a type position still fails to parse
with "Unknown compound type: ptr".

## 5. Correctness bugs worth fixing regardless of self-hosting

- **`and`/`or` don't short-circuit**, in both the VM and the WASM backend
  (`vm.rs` evaluates both operands unconditionally; `wasm.rs` compiles both
  operands then does a bitwise `i32.and`/`i32.or`). For a language whose whole
  pitch is formally-verifiable contracts, guard patterns like
  `(req (and (gt ptr 0) (mem.load32 ptr)))` are unsafe: if `ptr <= 0`, the
  `mem.load32` still executes and can throw an out-of-bounds error the
  contract was trying to prevent.
- **The WASM backend's `and`/`or` are also just wrong**, not merely
  non-short-circuiting: `i32.and`/`i32.or` are bitwise, not logical. `(and
  true 2)` isn't type-checkable today since the checker requires literal
  `Bool`, but any boolean produced by an opaque i32 value that isn't exactly 0
  or 1 (plausible once pointers/enums exist) would silently misbehave.
- **`Div` never checks for zero in the WASM backend** (only the VM does, via a
  Rust-side `if y == 0` check) — a WASM-compiled program dividing by zero traps
  with an opaque WASM runtime error instead of a diagnosable AIPL error.
- **`Literal::Str` compiles to `I32Const(0)`** in the WASM backend — every
  string literal is silently discarded. Combined with `SysPrint` compiling to
  `Nop`, a WASM-compiled AIPL program cannot ever meaningfully use a string.

~~`match_result`'s `collect_lets` doesn't recurse into `Call` args,
`MatchResult` bodies, or `Ok`/`Err`~~ — **RESOLVED**. `collect_lets` in
`src/compiler/wasm.rs` now handles all of these, alongside a separate fix for
loop induction variables never getting a wasm local slot at all (that one
doesn't just miss allocating a local — it silently drops the entire loop body
while leaking a value onto the wasm stack; fixed by registering the loop
variable in `collect_lets` and by making `Expr::Loop`'s codegen error loudly
instead of silently no-op-ing if a local is still somehow missing).

## 6. What's missing for AIPL to be a "full, extensible" language

- ~~No module/import system.~~ **RESOLVED, with a caveat.** `(import name)` /
  `(import name as alias)` works end-to-end: the parser accepts it, and
  `src/resolver.rs` flattens imported modules into one qualified-name
  (`module_name.fn_name`) program before the checker/VM/wasm backend ever
  runs — handling aliasing, transitive imports, diamond-dependency
  de-duplication, and circular-import detection. **The caveat**:
  `src/resolver.rs` is explicitly temporary Rust scaffolding, not part of the
  self-hosted toolchain — AIPL has no file I/O *opcode* usable from a
  compiled/portable program in a host-independent way for this purpose yet
  (the VM's new `fs.*` ops, §4, are real but Rust-side-only for now; nothing
  reads another `.aipl` file from *within* AIPL source). Once WASI file I/O
  exists for the wasm target, this module should be deleted and rewritten as
  real AIPL. Also worth knowing: the import system exists but is **underused**
  — see §2's note on `pipeline.aipl` and `sovereign_toolchain.aipl`, which
  duplicate (and in one case regress) the real tokenizer/parser instead of
  importing them.
- **No user-defined types.** No `struct`, `record`, or `enum`. The only
  compound types are `(arr T N)` and `(vec T N)` — fixed-size, single-element-
  type collections. There is no way to define e.g. a `User { id: i32, karma:
  i32 }` record; every "object" in the existing example programs (users,
  stories, AST nodes) is represented as raw offsets into linear memory with
  comments as the only documentation of the layout. No compiler anywhere
  checks that a memory layout comment matches what the code actually does.
- **No generics.** `(arr T N)` requires a literal `N`; there's no way to write
  a function generic over array length or element type. Every "container"
  algorithm has to be hand-specialized per size/type.
- **No first-class functions or closures**, despite the spec's own example
  (`PROMPT_GUIDE_FOR_AIS.md`, `hello_browser.aipl`) passing an inline `(fn
  [e:i32] -> void ...)` as a callback to `dom.on`. `parse_expr` has no grammar
  rule for a `fn` literal in expression position — only top-level named
  functions parse. This construct almost certainly doesn't parse today. (The
  new `thread.spawn` opcode works around this by naming its target function
  via a pointer+length into memory rather than a function value — a real,
  usable pattern, but a workaround, not first-class functions. **That
  workaround doesn't survive the import system**: the resolver (§6, below)
  only rewrites function names appearing as `(call ...)` syntax, so a
  `thread.spawn` target name stored as runtime bytes in memory is invisible
  to it. `aipl_src/thread_sync.aipl`'s own test works standalone but breaks
  if imported into another module, because its worker function gets renamed
  to `thread_sync.worker_increment` while the bytes it spawns still say
  `worker_increment` — see `PROGRESS.md` for the reproduction. A real fix
  needs either first-class function values or a name-resolution convention
  that's aware of the caller's own qualified prefix.)
- **No `break`/`continue`/early-return.** `loop`/`while` always run to
  completion of their bound or condition. `resolve_symbol_index` in
  `compiler.aipl` "finds" a match early but has no way to stop iterating.
- **No global/module-level constants or mutable state** — only function-local
  `let`. (The VM's `eval_expr` for `Set` *will* fall back to a `self.globals`
  map if the name isn't in local scope, but there's no syntax to declare or
  read a global explicitly, and the WASM backend has no globals section at
  all — this only works in the tree-walking interpreter. Several `aipl_src`
  files work around this with a "bump allocator state lives in a well-known
  memory cell" convention — see `memory.aipl`'s `aipl_heap_alloc` — which is a
  legitimate, reusable pattern for this language, not a bug.)
- **No visibility/namespacing beyond what imports now provide.** Within a
  single module, every function is still addressable by bare name with no
  privacy — the import system (above) namespaces *across* files, but there's
  no way to mark a function private to its own module.
- **No generic pattern matching** — `match_result` is hard-coded to the
  2-variant `Result` shape. There's no `match`/`case`/`switch` over arbitrary
  values or user-defined enums (which don't exist yet either).
- **No real arrays at runtime.** `Value::Array` exists in the VM's value enum
  but nothing ever constructs one — there's no array literal syntax, and
  `ArrGet`/`ArrSet` are two of the ops that silently return `0`/no-op (§4).
  Arrays are a documented type that doesn't work at all yet.
- **No real strings beyond parsing.** Strings survive as `Value::Str` in the
  tree-walking VM (used successfully by `SysPrint` there), but have zero
  representation in the WASM backend (§4, §5). There's no string
  concatenation opcode, no length/indexing/slicing, no UTF-8-aware operations
  — "str" today means "can be typed and printed by the interpreter only."
- **A real (if small) standard library has started to exist.** This used to
  say there was none. `aipl_src/memory.aipl` (a correct bump allocator + arena
  allocator, with genuine content-assertion tests), `aipl_src/file_io.aipl`,
  and `aipl_src/thread_sync.aipl` (both real as of this pass — see below) are
  legitimate reusable modules now. Still missing: a collections library, a
  string library, a math library beyond raw arithmetic opcodes. `stdlib::web`
  (Rust-side) is still a single hardcoded JS string
  (`get_browser_js_bridge`) — the "standard library" for the web target *is*
  JavaScript source code generated by Rust, the opposite of self-hosting;
  that part of the finding stands.
- **No source location tracking.** The tokenizer discards line/column
  information; every parser/checker error message reports only the offending
  token's value, not where in the file it came from. For a language pitched
  at AI-generated code, an AI fixing a reported error has to search the whole
  file for the mentioned symbol rather than jump to a location.
- **The "20-byte binary diagnostic record" system (`diagnostics.aipl`) isn't
  wired to anything.** The Rust parser/checker/VM all return plain `String`
  errors; nothing calls into `diagnostics.aipl`'s formatter.
- ~~No real concurrency.~~ **RESOLVED in the VM; still absent in the WASM
  backend.** `thread.spawn`/`thread.join` launch genuine OS threads
  (`std::thread`) sharing real linear memory (`Arc<Mutex<SharedMemory>>` in
  `src/vm.rs`), and `atomic.lock`/`atomic.unlock` are a real spinlock on that
  shared memory (not a side-table nothing waits on). Verified with 4 real
  threads each incrementing a shared counter 1000 times via `atomic.add`,
  landing on exactly 4000 — a result that requires the concurrency to be
  genuinely correct, not just structurally present. The WASM backend has none
  of this (§4) — it needs shared memory + wasi-threads, neither wired up yet,
  and explicitly refuses to compile programs using these ops rather than
  silently producing a broken binary.
- **No error/exception mechanism beyond the 2-variant `Result`.** No panics
  with unwinding, no typed error hierarchies — every function that can fail
  returns an `(ok/err ...)` and the caller must use `match_result`, which
  itself can't nest cleanly (each `match_result` hard-codes both arms' bodies
  inline, so composing several fallible calls means deeply nested
  `match_result`s, not a `?`-style short-circuit).
- **No package/dependency system at all** — no manifest format, no versioning,
  no way to reference a published AIPL library (the new import system, above,
  resolves by bare filename search only). Every "distribution" today is a
  single `.aipl` file or a hand-copied/imported source tree.
- **No formatter, linter, or language server.** For a language whose stated
  audience is AI agents generating code, tooling that gives fast, structured
  feedback on style/shape before a full compile would matter more than for
  human-authored code — right now the only feedback loop is a full
  parse+typecheck+(compile) cycle.

## 7. Bootstrapping status: what it would take to sever Rust/JS entirely

The README's own 3-stage plan (Stage 0: Rust bootstrap → Stage 1: self-hosted
AIPL compiler running in WASM → Stage 2: native ELF, zero dependencies) is the
right shape. Where it actually stands:

1. **Rust is installed** (via WSL — Cargo isn't on the Windows PATH directly;
   see [PROGRESS.md](PROGRESS.md) for the exact invocation). This used to be a
   hard blocker (nothing could be built or verified at all); it no longer is.
2. The self-hosted compiler's tokenizer and parser are real (§2). **Codegen
   (`emit_wasm_binary`) is the single largest remaining piece of work** —
   walking the real AST tree `parse_ast` now produces and emitting real WASM
   instructions for it, replacing the hardcoded stub.
3. A **`wasm_runtime.aipl`** (an AIPL-native interpreter for the specific,
   small subset of WASM instructions AIPL's own compiler emits) plus a fully
   built-out `elf_emitter.aipl` would be the genuinely dependency-free way to
   both compile *and run* AIPL without wasmtime/Node/a browser — but note the
   explicit decision made this session: for the "as fast as native/assembly"
   target, the chosen near-term direction is **WASM + an external AOT
   compiler** (e.g. Cranelift via wasmtime) rather than a hand-rolled native
   backend. That's a deliberate trade of full sovereignty for reaching real
   native performance sooner, and it deprioritizes further investment in
   `elf_emitter.aipl`/`wasm_runtime.aipl` unless that decision gets revisited.
   `elf_emitter.aipl` does now contain several *correct* individual x86-64
   syscall-emission helpers (`emit_x86_sys_open/read/write/close/clone`) added
   in a later pass — real machine code, verified by inspection — but they are
   not called from anywhere in the codebase yet; they're real primitives
   sitting unused, not a working native pipeline.
4. `tests/test_v2.rs`'s weak self-hosting tests (§2) still need rewriting to
   exercise varied, non-trivial input — the scope of this grew rather than
   shrank, since a later pass added more tests following the same weak
   pattern (`test_e2e_sovereign_pipeline_bootstrap`) rather than fixing the
   original ones.

## 8. Architectural bets still needed before "massively more modules" is safe

Written ~12 hours into the project, deliberately: these are the criticisms
worth tracking now precisely *because* they're architectural rather than
bugs — the earlier this is written down, the less gets built on top of an
assumption that later has to be ripped out. Everything here is a real,
identified gap, not speculation; several items below are cross-references to
existing bullets in §6 rather than new findings, consolidated here because
together they answer one question: can this scale to many modules from many
authors, not just to more features in one file?

1. **No user-defined types is the load-bearing gap, not a nice-to-have.**
   §6 already lists "no structs/records/enums" and "no generics" as separate
   bullets; the reason to call it out again here is what happens as module
   *count* grows, not just feature count. Every piece of structured data that
   crosses a module boundary today is "some bytes at fixed offsets," with the
   layout documented only in a comment, enforced by nothing. That's tolerable
   for one author's one file. It gets *worse*, not better, as more modules
   from more authors need to agree on shared layouts — there's no type-level
   contract to catch drift, only tribal knowledge. This is the one change
   most likely to require touching the parser, checker, VM, and wasm backend
   simultaneously (a real "serious architectural change," not a bolt-on).

2. **The self-hosted compiler's own scratch memory is fixed-size and
   unchecked.** New finding, not yet in §2/§4: `aipl_src/codegen.aipl` uses
   hardcoded, fixed-capacity regions for its keyword table (20000), function
   table (30000), locals table (41000), and per-function code-emission
   scratch (60000/61000/70000...), plus single memory cells for parser/
   allocator state (40500, 40600). None of these grow dynamically or check
   bounds. This works for the test programs compiled so far; it will
   *silently corrupt memory* rather than error on a large enough program or
   module count. This needs fixing (dynamic growth, or at minimum generous
   capacity plus explicit bounds checks with a loud error) before "massive"
   module counts are even mechanically safe to compile, independent of any
   language-design question.

3. **Nothing stops duplication even though imports exist.** Already
   documented in §2 as a concrete incident (`pipeline.aipl` reimplementing,
   and regressing, the tokenizer that already existed in `compiler.aipl`,
   despite `(import compiler)` being available). Worth restating as a
   pattern, not a one-off: the import mechanism being *possible* doesn't
   make reuse the default behavior, for a human or an AI. At small module
   counts this is a nuisance; at "massive" module counts it's how an
   ecosystem ends up with a dozen slightly-different half-working tokenizers.
   Closing this needs more than a language feature — some combination of
   discoverability (a way to answer "does a module for X already exist?"),
   convention, and possibly lint tooling (§6 already lists "no formatter,
   linter, or language server" as a gap).

4. **No versioning or dependency resolution** (§6 has this already): the
   import resolver does a bare filename search with no version concept. Two
   modules wanting different versions of a shared dependency have no way to
   express that. Not urgent at current scale (one author, ~10 modules); real
   at the scale this question is asking about.

5. **No visibility/privacy** (§6 has this already): every function in every
   module is globally addressable via its qualified name. Fine for a handful
   of modules; at scale it means every module's internal helpers pollute the
   same flat namespace as its public API, with nothing distinguishing them.

None of these are reasons not to build more modules now — they're reasons to
expect a real migration (likely breaking) once structs/generics land, and to
resist the temptation to route around that by hand-rolling "yet another"
memory-layout convention per module in the meantime.

## 9. On the longer-term ambition (standalone browser / PDF viewer in pure AIPL)

Worth naming honestly: reaching a point where an AI can generate something
like a full browser engine or PDF viewer as pure AIPL source and get a
self-contained executable out the other end is a real, coherent direction for
this project — but it sits behind essentially everything in §6, plus
substantial domain-specific work that has nothing to do with AIPL as a
language:

- A **browser** needs, at minimum: an HTML parser, a CSS parser and cascade/
  layout engine, a JS engine (or a decision to not support JS), a rendering
  pipeline, font shaping and rasterization, image codecs (PNG/JPEG/etc.), a
  network stack (sockets/TLS), and — critically — a *host* that can open a
  window and hand the program a pixel buffer or GPU surface. WASM itself has
  no windowing or graphics capability; it can only call whatever the host
  environment provides as imports. A "standalone WASM executable" for a
  browser still needs a minimal host runtime underneath it (a window +
  event loop + GPU/framebuffer surface) — that host doesn't have to be Rust or
  JS, but it can't be avoided by staying in WASM alone. Native ELF (once
  built out) sidesteps this by talking to the OS directly, but "talking to
  the OS directly" for windowing/graphics means implementing an X11/Wayland/
  Win32/Cocoa client from scratch per platform, which is its own enormous
  effort independent of AIPL's compiler maturity.
- A **PDF viewer/editor/generator** is more tractable as a first target
  (parsing a well-specified binary format, rendering vector graphics and text
  is a much smaller and more self-contained problem than an HTML/CSS/JS
  engine), and would be a reasonable "prove the language can do real systems
  work" milestone once §6's structs/arrays/strings exist — still a
  multi-month project, but a realistic one, and it wouldn't need a windowing
  host if it only needs to rasterize to a memory buffer.

Neither is a near-term goal. The realistic path there runs through §6 (types,
real strings/arrays, a standard library) and §7 (a compiler that actually
compiles, plus a native runner), not around them.
