# AIPL Language Gap Analysis

Companion document to [AIPL_SPEC.md](AIPL_SPEC.md) and [PROMPT_GUIDE_FOR_AIS.md](PROMPT_GUIDE_FOR_AIS.md).
Where those describe what AIPL is supposed to do, this describes what it actually
does today, verified against the real implementation and real compiled output
(not just the source code). Written for AI agents and contributors picking up
self-hosting work on this repo.

Verification method: no Rust toolchain, Node, or WASM runtime was available in
the environment this was written in. `wasmtime` (Python bindings) was installed
via `pip install wasmtime` to actually load and execute the `.wasm` files
already committed to this repo, and to call their exported functions with real
arguments. Everything marked **VERIFIED** below was checked this way, not
inferred from reading source.

---

## 1. Empirical findings: committed `.wasm` binaries

Every `.wasm` file in the repo was loaded with a real, spec-compliant WASM
runtime (`wasmtime`). Five of seven fail to even instantiate:

| File | Result |
|---|---|
| `aipl_compiler.wasm` | Loads. Exports `tokenize`, `parse_ast`, `emit_wasm_binary`, `compile_aipl`, etc. |
| `examples/hn_clone/hn_clone.wasm` | Loads. Exports `get_rank`, `upvote_score`, `calculate_rank_score`, `init_hn_header`, `main`. |
| `aipl_db.wasm` | **FAILS.** `type mismatch: expected i32 but nothing on stack` at byte offset 327. |
| `aipl_sovereign_toolchain.wasm` | **FAILS.** `type mismatch: values remaining on stack at end of block` at offset 1107. |
| `examples/aipl_database/aisql_engine.wasm` | **FAILS.** `type mismatch: expected i32 but nothing on stack` at offset 232. |
| `examples/hn_clone/hn_full_engine.wasm` | **FAILS.** `type mismatch: expected i64, found i32` at offset 221. |
| `hn_cli.wasm` | **FAILS.** `type mismatch: expected i64, found i32` at offset 241. |

**This means the HN clone demo (`hn_clone.html`) was never actually able to run
its security engine in any spec-compliant WASM host, including real browsers**
(V8/SpiderMonkey/JSC enforce the same validation rules `wasmtime` does) — not
just "unverified," but rejected at load time. Hand-decoding the `hn_full_engine.wasm`
bytecode around offset 221 shows the root cause: the `(^ ...)` (XOR) operation in
`hash_password` was encoded as opcode `0x85` (`i64.xor`) operating on two `i32`
locals, instead of `0x73` (`i32.xor`). The **current** `src/compiler/wasm.rs`
source correctly emits `Instruction::I32Xor` for `OpCode::BitXor` — so this
binary was built by an older/different version of the compiler and never
rebuilt. This is a concrete instance of the general problem in critique #7
(committing compiled binaries instead of building from source): the artifacts
in the repo no longer match the source that supposedly produced them, and
nothing caught the drift because nothing here can currently rebuild them to
compare.

**Practical consequence of the JS decoupling done earlier this session:** the
fix to `hn_runner.js` (removing JS fallback math, requiring the real WASM
engine) means the app will now correctly report "AIPL Wasm engine not loaded"
when this broken binary fails to instantiate, instead of silently limping along
on JS math. That is the correct behavior given the binary is genuinely broken —
but the binary itself still needs to be regenerated once a working compiler is
available.

## 2. Empirical findings: the "self-hosted compiler" is provably a stub

`aipl_compiler.wasm`'s `compile_aipl(source_ptr, source_len, out_ptr)` was
called with two completely different inputs written into linear memory:

- `"(module a (fn add [x:i32 y:i32] -> i32 (+ x y)))"`
- `"totally different garbage input !!! 12345 #####"`

**Both calls returned exactly 42 bytes of byte-for-byte identical output**
(`0061736d0100000001070160027f7f...`), a fixed WASM module exporting a `main`
function that does `local.get 0; local.get 1; i32.add; end`. This is not a
matter of interpretation — `aipl_src/compiler.aipl`'s `tokenize` only reacts to
`(`/`)` characters, `parse_ast` never reads the tokens it's given (lines 76-85:
it writes 4 hardcoded constants regardless of input), and `emit_wasm_binary`
hardcodes an entire fixed module byte-by-byte, reading only one dynamic value
(`(mem.load32 (+ ast_ptr 12))`, the opcode) out of the AST buffer it just
hardcoded itself in the previous step. **The compiler does not compile.**

`tests/test_v2.rs`'s self-hosting tests only assert the output starts with the
WASM magic bytes and is `>20` bytes long — exactly the constants this stub
always emits. They would pass on empty or garbage input and currently give
false confidence that self-hosting works.

## 3. A real but subtle finding: no byte-granularity memory op

Every AIPL byte-buffer-writer (`wasm_emitter.aipl`, `compiler.aipl`,
`elf_emitter.aipl`) builds output one byte at a time using `mem.store32` at
consecutive offsets, e.g.:

```lisp
(mem.store32 (+ ptr 0) 0)    ;; \0
(mem.store32 (+ ptr 1) 97)   ;; a
(mem.store32 (+ ptr 2) 115)  ;; s
```

`mem.store32` writes 4 little-endian bytes, not 1, so this writes overlapping
4-byte spans at every offset. Tracing it carefully: this happens to produce the
*correct* byte sequence, because each store's low byte (the intended value) is
always written last for its own address by construction of increasing offsets,
and every stored "byte value" here is `< 256` so its upper 3 bytes are always
zero. **It works, but by accident of convention, not by design** — there is no
compiler-enforced guarantee that a value passed to this pattern stays under
256, no `mem.store8`/`mem.load8` opcode, and no documentation anywhere that
this is the required idiom. A value that leaks through at 256 or above (e.g.
from an arithmetic bug) would silently corrupt the following 3 bytes with no
error. This is a real, missing primitive: **byte-level (8-bit) memory
load/store**, needed for any serious binary-format, string, or I/O work, not
just a convention issue.

## 4. Type/opcode coverage matrix (parser → checker → VM → WASM backend)

Cross-referencing `src/ast.rs`'s `OpCode` enum (44 variants) against every
place it's consumed:

| OpCode | Parsed | Type-checked | VM (`vm.rs`) | WASM (`wasm.rs`) |
|---|---|---|---|---|
| Add, Sub, Mul, Div | Yes | Yes | Yes | Yes |
| **Mod** (`%`) | Yes | Yes | **No** (falls to `Int(0)`) | **No** (falls to `Nop`) |
| BitXor, Shl, Shr, BitAnd, BitOr | Yes | Yes | Yes | Yes |
| MemLoad32/64, MemStore32/64 | Yes | Yes | Yes | Yes |
| **MemLoadF32/F64, MemStoreF32/F64** | Yes | Yes | **No** | **No** |
| MemAlloc | Yes | Yes | Yes (bump allocator, never frees) | **No** |
| MemFree | Yes | Yes | No-op (documented as no-op) | **No** |
| AtomicAdd | Yes | Yes | Yes | **No** |
| **AtomicCas** | Yes | Yes | **No** | **No** |
| AtomicLock/Unlock | Yes | Yes | Yes (a `HashMap<usize,bool>`, not a real mutex — see §6) | Compiles (marked void) but no real instruction emitted beyond whatever falls through |
| Eq, Neq, Lt, Lte, Gt, Gte | Yes | Yes | Yes | Yes |
| And, Or | Yes | Yes | Yes (**does not short-circuit** — see §5) | Yes (compiles to bitwise `i32.and`/`i32.or`, also non-short-circuiting, and wrong if operands aren't exactly 0/1) |
| Not | Yes | Yes | Yes | Yes |
| **VecDot, MatMul** | Yes | Yes (fixed return types) | **No** | **No** |
| **ArrGet, ArrSet** | Yes | **No** (falls through to a default `Type::I32`, i.e. untyped) | **No** | **No** |
| **DomElem, DomMount, DomAppend, DomOnEvent, WebAlert** | Yes | Yes | **No** | **No** (silent `Nop`) |
| SysPrint | Yes | Yes | Yes | **No** (silent `Nop` — a compiled program can never print) |
| **SysTime, SysExit** | Yes | Yes | **No** | **No** |

17 of 44 opcodes (39%) silently return `0`/no-op in the VM instead of running
or erroring. Roughly the same set silently `Nop` in the WASM backend. This
means any AIPL program using vectors, matrices, arrays, DOM, `sys.exit`,
`sys.time`, float memory ops, or CAS **executes without any error message and
produces meaningless results** — this is worse than a crash, because nothing
tells the author (human or AI) that anything went wrong.

Type-level gaps in `src/parser.rs::parse_type` (only `i32/i64/f32/f64/bool/str/
void/arr/vec` are parseable, despite `Type::Ptr`, `Type::Fn`, and
`Type::ResultType` existing in the AST): a source file using `(ptr i32)` or
`(fn (i32) -> i32)` in a type position — both in the grammar in
`AIPL_SPEC.md` §2 — fails to parse with "Unknown compound type: ptr".

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
- **`match_result`'s `wasm.rs::collect_lets` doesn't recurse into `Call` args,
  `MatchResult` bodies, or `Ok`/`Err`** — `let`s declared inside those
  constructs never get a WASM local slot allocated, which is a real crash
  waiting to happen (`Wasm Codegen: Unbound local variable`) for any program
  using those constructs together, not just a style nit.
- **`Literal::Str` compiles to `I32Const(0)`** in the WASM backend — every
  string literal is silently discarded. Combined with `SysPrint` compiling to
  `Nop`, a WASM-compiled AIPL program cannot ever meaningfully use a string.

## 6. What's missing for AIPL to be a "full, extensible" language

These aren't overclaims to correct — they're just absent, and are what
"extensible" would require:

- **No module/import system.** A file can't reference another file's
  functions. This is why `sovereign_toolchain.aipl` is a 407-line hand
  copy-paste of 5 other files. Almost everything else on this list is blocked
  or made much harder without this, since without it there's no way to build a
  standard library out of small composable files.
- **No user-defined types.** No `struct`, `record`, or `enum`. The only
  compound types are `(arr T N)` and `(vec T N)` — fixed-size, single-element-
  type collections. There is no way to define e.g. a `User { id: i32, karma:
  i32 }` record; every "object" in the existing example programs (users,
  stories, AST nodes) is represented as raw offsets into linear memory with
  comments as the only documentation of the layout (`;; AST Node Structure:
  [node_kind:i32, param_count:i32, ...]` in `compiler.aipl`). No compiler
  anywhere checks that a memory layout comment matches what the code actually
  does.
- **No generics.** `(arr T N)` requires a literal `N`; there's no way to write
  a function generic over array length or element type. Every "container"
  algorithm has to be hand-specialized per size/type.
- **No first-class functions or closures**, despite the spec's own example
  (`PROMPT_GUIDE_FOR_AIS.md`, `hello_browser.aipl`) passing an inline `(fn
  [e:i32] -> void ...)` as a callback to `dom.on`. `parse_expr` has no grammar
  rule for a `fn` literal in expression position — only top-level named
  functions parse. This construct almost certainly doesn't parse today.
- **No `break`/`continue`/early-return.** `loop`/`while` always run to
  completion of their bound or condition. `resolve_symbol_index` in
  `compiler.aipl` "finds" a match early but has no way to stop iterating.
- **No global/module-level constants or mutable state** — only function-local
  `let`. (The VM's `eval_expr` for `Set` *will* fall back to a `self.globals`
  map if the name isn't in local scope, but there's no syntax to declare or
  read a global explicitly, and the WASM backend has no globals section at
  all — this only works in the tree-walking interpreter.)
- **No visibility/namespacing** — every function in a module is globally
  addressable by bare name; once modules exist, nothing will stop name
  collisions across files.
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
- **No real standard library.** `stdlib::sys` is wall-clock time only.
  `stdlib::web` is a single hardcoded JS string (`get_browser_js_bridge`) —
  i.e., the "standard library" for the web target *is* JavaScript source code
  generated by Rust, the opposite of self-hosting. There's no collections
  library, no string library, no math library (beyond raw arithmetic
  opcodes) — every example program hand-rolls everything (FNV hashing,
  ranking formulas) from scratch because there's nothing to import.
- **No source location tracking.** The tokenizer discards line/column
  information; every parser/checker error message reports only the offending
  token's value, not where in the file it came from. For a language pitched
  at AI-generated code, an AI fixing a reported error has to search the whole
  file for the mentioned symbol rather than jump to a location — this
  directly undercuts the "excellent diagnostics for AI agents" pitch, and
  matters more here than in a typical human-authored-code compiler.
- **The "20-byte binary diagnostic record" system (`diagnostics.aipl`) isn't
  wired to anything.** The Rust parser/checker/VM all return plain `String`
  errors; nothing calls into `diagnostics.aipl`'s formatter. The one polished
  piece of the aspirational diagnostics story is disconnected from the actual
  error path.
- **No real concurrency**, despite "atomic swarm synchronization" being a
  headline pitch. `AtomicLock`/`AtomicUnlock` in the VM just flip a boolean in
  a `HashMap` — there's no actual thread, no blocking, and nothing enforces
  mutual exclusion (two "locked" sections can still interleave freely since
  the VM is single-threaded and nothing checks the lock before proceeding).
  There is no thread/task primitive of any kind, in the VM or the WASM
  backend.
- **No error/exception mechanism beyond the 2-variant `Result`.** No panics
  with unwinding, no typed error hierarchies — every function that can fail
  returns an `(ok/err ...)` and the caller must use `match_result`, which
  itself can't nest cleanly (each `match_result` hard-codes both arms' bodies
  inline, so composing several fallible calls means deeply nested
  `match_result`s, not a `?`-style short-circuit).
- **No package/dependency system at all** — no manifest format, no versioning,
  no way to reference a published AIPL library. Every "distribution" today is
  a single `.aipl` file or hand-copied source.
- **No formatter, linter, or language server.** For a language whose stated
  audience is AI agents generating code, tooling that gives fast, structured
  feedback on style/shape before a full compile would matter more than for
  human-authored code — right now the only feedback loop is a full
  parse+typecheck+(compile) cycle.

## 7. Bootstrapping status: what it would take to sever Rust/JS entirely

The README's own 3-stage plan (Stage 0: Rust bootstrap → Stage 1: self-hosted
AIPL compiler running in WASM → Stage 2: native ELF, zero dependencies) is the
right shape. Where it actually stands:

1. **Rust is not installed anywhere on this machine** — not just missing from
   PATH; there's no `.cargo`/`.rustup` directory. Nothing in this repo can
   currently be built, and no existing test (`cargo test`) can currently be
   run, by anyone working on this machine. This needs to be resolved (install
   Rust, even if only temporarily as the one-time Stage 0 bootstrap tool)
   before any further self-hosting work can be verified rather than written
   blind.
2. The self-hosted compiler (`aipl_src/compiler.aipl`) needs an actual
   tokenizer, parser, and code generator that consume their real input — not
   the current hardcoded-output stub (§2). This is the single largest and
   most important piece of work, and it's large enough to warrant its own
   pass rather than being bundled with everything else here.
3. A **`wasm_runtime.aipl`** — an AIPL-native interpreter for the specific,
   small subset of WASM instructions AIPL's own compiler emits (i32/i64
   const, local get/set, the arithmetic/comparison/memory ops already listed
   in §4, `block`/`loop`/`if`/`br`/`br_if`/`call`/`end`) — is a genuinely good
   next step, and pairs naturally with fixing the compiler stub. If this is
   compiled to native ELF (once `elf_emitter.aipl` is built out — currently it
   only handles one fixed "load 2 constants, do 1 op, exit" program, per the
   original critique), the result is a real, dependency-free way to *run*
   AIPL-compiled WASM without wasmtime, Node, or a browser: `aipl_compiler`
   (native binary) compiles `foo.aipl` → `foo.wasm`, then `wasm_runtime`
   (native binary) executes `foo.wasm` directly, and neither step touches
   Rust, JS, or any external runtime ever again. That's the actual "sever all
   dependencies" finish line for the execution side. It only needs to
   interpret the small opcode subset AIPL itself emits, not the full WASM
   spec — full WASM (SIMD, multi-value, reference types, exception handling,
   threads) is a much bigger undertaking that isn't needed here.
4. Rewriting `tests/test_v2.rs`'s self-hosting tests so they exercise varied,
   non-trivial input (§2) should happen alongside item 2, not after — the
   current tests would give false confidence again on the next stub.

## 8. On the longer-term ambition (standalone browser / PDF viewer in pure AIPL)

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
  work" milestone once §6's structs/arrays/strings/modules exist — still a
  multi-month project, but a realistic one, and it wouldn't need a windowing
  host if it only needs to rasterize to a memory buffer.

Neither is a near-term goal. The realistic path there runs through §6 (types,
modules, real strings/arrays, a standard library) and §7 (a compiler that
actually compiles, plus a native runner), not around them.
