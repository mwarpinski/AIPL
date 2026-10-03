# AIPL Structural Audit

Audit date: 2026-09-17. Tree at commit `c97a10c`. Every claim below is anchored to a file and line in this repo; nothing under `target/` was consulted.

Companion documents: [LANGUAGE_GAPS.md](LANGUAGE_GAPS.md) (what the language does not yet do), [PROGRESS.md](PROGRESS.md) (what has been built and verified). This document is the execution roadmap: what is structurally sound, what is debt, what will break at scale, and the ordered list of tasks (with ready-to-run agent prompts) to fix it.

## Status of the findings below (re-checked 2026-10-01)

Sections 1–3 are the audit as written on 2026-09-17, and their line references point at that tree. Every finding was re-checked against the code on 2026-10-01 (after P14); the table gives its current state. Findings the original audit did not cover are in section 3b.

| Finding | Status |
|---|---|
| B1 silent catch-alls | Fixed (P2). `tests/test_opcode_conformance.rs` covers every opcode |
| B2 integer semantics diverge | Fixed (P3, i64 task) |
| B3 type-system holes | Fixed: strings compile (P6), `set!` is void and `match_result` binds real types (P7), `if` block types follow branch types, pointers and arrays are strictly typed `(ptr S)` / `(arr T)`, and `(fn [..] -> r)` types parse (P10) |
| B4 scoping undefined | Fixed (P7) |
| B5 `inv` never evaluated, contracts absent from wasm, `verify` overclaims | Partly fixed (2026-10-01): `aipl verify` now says contracts are type-checked, not proven, and a contract failure prints the contract as source with its position and the arguments. Still open: `inv` is never evaluated and compiled wasm has no contracts |
| B6 two allocators | Fixed (P5) |
| B7 fabrications in tree | Fixed (P1) |
| B8 duplicated code | Fixed: `wasm_emitter.aipl` is in `attic/`; `compiler.aipl` holds the only `encode_u32`/`emit_header` |
| B9 byte emission via `mem.store32` | Fixed (2026-10-01): `encode_u32` writes bytes with `mem.store8`; `emit_header` writes two aligned words |
| B10 ELF backend | Retired (P1 quarantine, P13 strategy in `docs/NATIVE_TARGET.md`) |
| B11 no positions | Fixed (P4) for parse and check errors; most VM runtime errors still have none (LANGUAGE_GAPS.md 1) |
| B12 tokenizer edge cases | Fixed (P4, P6 escapes) |
| B13 interpreter clones the body on every call | Fixed (2026-10-01): function bodies are shared (`Arc<FnDef>`), about 20% faster on the self-hosted toolchain. The VM is still a tree-walker with string-keyed scopes and is far slower than the compiled toolchain (N10) |
| B14 binary AST is serde layout | Fixed (P12: deleted) |
| B15 threads by name | Fixed (P10: `thread.spawn` takes a function reference). The VM's per-thread `globals` map was never written and has been removed |
| U1 no aggregate types | Fixed (P8) |
| U2 fixed-address global state | Fixed (P5) |
| U3 three semantics | Addressed: wasm semantics are the spec, differential tests compare the VM against wasmtime, and the self-hosted output must equal the Rust backend's byte for byte, which is the diverse double-compilation check the audit asked for. The toolchain also reproduces itself under wasm (P14) |
| U4 compiled code cannot do I/O | Fixed (P6; command line and environment in P14) |
| U5 control flow too poor | Fixed (P11) |
| U6 tests certify fabrications | Fixed (P1); recurred in P8 and was caught on re-verification. Self-tests that only checked "returned a number" in codegen.aipl now check their output (N9) |
| U7 memory has no growth or bounds contract | Mostly fixed: allocation grows memory automatically up to 1024 pages and bounds are enforced in both backends. Still open: `mem.free` is a no-op (N6) |
| U8 nothing versioned | Deferred (P12 note: until there are packages or a second toolchain) |

The order of upcoming work is in PROGRESS.md ("Next steps"), not section 4 below, which is the completed P1–P14 history. **Still open, in order of risk:** N4 (compiled array indexing is unchecked), N3 (the checker exists only in Rust), B5 (contracts), N6 (no `free`).

---

## 1. THE GOOD

**As of 2026-10-01**, the strengths that matter most are ones the original audit could only hope for:

- **Wasm semantics are the specification, and that is tested.** `tests/test_differential.rs` runs the same programs in the VM and in wasmtime, including every example and every standard-library module, and fails on any disagreement.
- **Three code paths, one output.** The Rust backend, the self-hosted backend run in the VM, and the self-hosted backend compiled to wasm all emit identical bytes, and the whole self-hosted toolchain (driver, resolver, codegen, compiler, std) compiled to wasm recompiles itself to exactly itself in about half a second.
- **The type system catches the mistakes LLMs make.** Strict `(ptr S)` / `(arr T)` types with explicit casts, namespaced structs, mandatory annotations, positioned diagnostics, and a checker that rejects misplaced `return`/`break`.
- **Tests are checked for teeth.** New self-tests are confirmed to fail when the code under test is broken on purpose (PROGRESS.md records the mutations).

The original section follows.


**The grammar is the asset.** One S-expression form per construct, `(call f …)` syntactically distinct from `(op …)`, contracts as positional prefix forms. The Rust parser is 575 lines with zero lookahead beyond one token ([parser.rs:159-166](src/parser.rs#L159-L166)), and the same grammar has already been re-implemented in AIPL itself (`tokenize`/`parse_node` in [compiler.aipl:131-277](aipl_src/compiler.aipl#L131-L277)) and validated against real inputs. That is the self-hosting bet paying off early.

**Contracts are enforced, not decorative.** `req` runs before the body and `ens` runs with `res` bound after it ([vm.rs:113-145](src/vm.rs#L113-L145)); the checker demands `Bool` for every contract ([checker.rs:37-55](src/checker.rs#L37-L55)). Contract failure is a hard error with the expression printed.

**The concurrency model is correct at the primitive level.** Linear memory plus the bump cursor is one `Arc<Mutex<SharedMemory>>` shared by every OS thread ([vm.rs:24-37](src/vm.rs#L24-L37)); atomics do the full RMW under that single lock; the spinlock releases the mutex between attempts and yields ([vm.rs:478-497](src/vm.rs#L478-L497)), so a lock holder is never starved of the ability to unlock. `thread_sync.aipl` proves 4×1000 = 4000 across real threads.

**Host ABI already shaped like WASI.** `fs.open`/`fs.delete`/`thread.spawn` take `(ptr, len)` into linear memory ([vm.rs:613-617](src/vm.rs#L613-L617)), so the signatures survive the move to `path_open`/`fd_read` imports unchanged.

**`is_void_expr` tracks codegen, not the type system** ([wasm.rs:418-461](src/compiler/wasm.rs#L418-L461)) and `compile_stmt` drops every effect-position value ([wasm.rs:161-167](src/compiler/wasm.rs#L161-L167)). That is the right invariant for a stack machine and it is stated explicitly.

**codegen.aipl produces validated wasm.** Instruction-level codegen for `let/set!/if/while/loop/call/binops/memops` written in AIPL, checked by wasmtime: `add(3,4)=7`, `compute` matches the VM on three inputs. The "state in a well-known memory cell" pattern (`40600` compile-error flag, `40500` locals count) is a legitimate substitute for globals in a language without them.

**Import resolution is honest.** Flat `name.fn` qualification, alias-only-local, diamond dedup and cycle detection ([resolver.rs:47-53, 87-96](src/resolver.rs#L47-L96)), and a header that says exactly when it should be deleted.

**Test runner contract is minimal and right.** `aipl test` invokes an entrypoint that returns a failure count and maps it to exit code ([main.rs:91-118](src/main.rs#L91-L118)); pass/fail logic lives in AIPL ([test_suite.aipl](aipl_src/test_suite.aipl)), with an explicit refusal to import unverified modules.

**`LANGUAGE_GAPS.md` and `PROGRESS.md` tell the truth.** Rare, and the only reason an agent can currently distinguish real from fake in this tree.

---

## 2. THE BAD

**B1. Silent catch-all arms are the root anti-pattern.** Four of them, in the four load-bearing files:
- VM: `_ => Ok(Value::Int(0))` ([vm.rs:781](src/vm.rs#L781))
- wasm: `_ => Nop` twice ([wasm.rs:356-358](src/compiler/wasm.rs#L356-L358), [411-413](src/compiler/wasm.rs#L411-L413))
- checker: `_ => Ok(Type::I32)` ([checker.rs:261](src/checker.rs#L261))
- type lowering: `_ => ValType::I32` ([wasm.rs:153](src/compiler/wasm.rs#L153))

Consequences today: `%` parses ([parser.rs:505](src/parser.rs#L505)), type-checks ([checker.rs:160](src/checker.rs#L160)), evaluates to `0` in the VM, and compiles to `nop` in wasm, which leaves the stack short by one and fails validation. Same for `sys.time`, `sys.exit`, `arr.get`, `arr.set`, `vec.dot`, `matmul`, every `dom.*`, `mem.alloc`, `mem.free`, `atomic.*`, `sys.print`, and `match_result`/`ok`/`err` in the wasm backend. `(let p:i32 (mem.alloc 64))` compiles to `nop; local.set` — invalid module, no error.

**B2. Integer semantics diverge between VM and wasm.** `Value::Int(i64)` ([vm.rs:10](src/vm.rs#L10)) does 64-bit arithmetic for a type declared `i32`; wasm does 32-bit wrapping. `*i as i32` truncates literals ([wasm.rs:173](src/compiler/wasm.rs#L173)). `shr` is arithmetic in both, so `encode_u32` ([codegen.aipl:605](aipl_src/codegen.aipl#L605) family, [wasm_emitter.aipl:8-26](aipl_src/wasm_emitter.aipl#L8-L26)) on any value ≥ 2³¹ terminates in the VM (i64 positive) and loops forever compiled (sign-extending). No unsigned ops exist. The loop bound bug already found (`I32GeS` vs inclusive VM) was the first of this class, not the last.

**B3. The type system has holes the checker papers over.**
- `Str` literals compile to `i32.const 0` ([wasm.rs:181-183](src/compiler/wasm.rs#L181-L183)); `Str` exists only in the interpreter.
- `Ptr`, `Fn` in [ast.rs:12,16](src/ast.rs#L12-L16) are unparseable ([parser.rs:277-312](src/parser.rs#L277-L312)).
- `ok`/`err` hardcode the other arm as `I32` ([checker.rs:265,269](src/checker.rs#L265-L269)); `match_result` binds both arm variables as `I32` regardless of the actual result type ([checker.rs:274,281](src/checker.rs#L274-L281)) — a `(ok 1.5)` matched as `i32` passes the checker.
- `if` whose branches are `set!` has AIPL type = variable type but pushes nothing in wasm ([wasm.rs:206-210](src/compiler/wasm.rs#L206-L210)); users must write `(block (set! done true) 0)`. Codegen is leaking into source. An `if` with `i64`/`f64` branches gets `BlockType::Result(I32)` — invalid.
- `set!` on an undefined name creates a global silently in the VM ([vm.rs:176-177](src/vm.rs#L176-L177)); the checker catches it only when run.

**B4. Scoping is undefined and the three implementations disagree.** VM `let` writes into one flat per-call map ([vm.rs:167-171](src/vm.rs#L167-L171)); `match_result` arms clone the scope so `set!` inside an arm is lost ([vm.rs:245,255](src/vm.rs#L245-L255)); `loop` in the checker uses a cloned env ([checker.rs:126](src/checker.rs#L126)) but the VM writes the loop var into the parent scope ([vm.rs:209](src/vm.rs#L209)); wasm gives every name one function-wide local and dedups by name ([wasm.rs:50-57](src/compiler/wasm.rs#L50-L57)), so two `let x` of different types in one function share a slot.

**B5. `inv` is parsed and never evaluated** ([parser.rs:272](src/parser.rs#L272), no consumer). Contracts are stripped from wasm entirely (wasm.rs never reads `f.contracts`). `aipl verify` prints "contracts verified" after only type-checking ([main.rs:89](src/main.rs#L89)).

**B6. Two allocators, one memory, no knowledge of each other.** `mem.alloc` bumps `heap_ptr` starting at 1024 ([vm.rs:46](src/vm.rs#L46)); `aipl_heap_alloc` bumps `mem[0]` starting at 8192 ([memory.aipl:9-18](aipl_src/memory.aipl#L9-L18)). codegen.aipl's tables at 20000/30000/40500/41000 sit inside the region `aipl_heap_alloc` will hand out after ~12KB of allocations. Memory is a hard 1MB ([vm.rs:45](src/vm.rs#L45)); `mem.free` is a no-op ([vm.rs:429](src/vm.rs#L429)).

**B7. Load-bearing fabrications are still in tree and still tested.**
- [sovereign_toolchain.aipl](aipl_src/sovereign_toolchain.aipl): `fs_open` returns `10`/`11` (L456-461), `fs_read`/`fs_write` return `count` with no I/O (L463-475), `thread_spawn_sync` returns `101` (L527-532), `tokenize` counts parens only (L222-234), `test_e2e_sovereign_pipeline` "passes" on `(gt wasm_len 30)` (L587).
- [pipeline.aipl:43](aipl_src/pipeline.aipl#L43) returns `4` tokens when there are zero; L211 sets `p_flags=7` (RWX).
- [compiler.aipl:328](aipl_src/compiler.aipl#L328) reads the opcode from `ast_ptr+12` — that is the `next_sibling` field; L362-370 "compiles" to ELF by writing six words and returning `120`.
- [tests/test_v2.rs](tests/test_v2.rs) lines 88-275: seven green tests (`test_self_hosted_wasm_emitter`, `test_dual_target_native_elf_emitter`, `test_e2e_sovereign_pipeline_bootstrap`, `test_sovereign_wasm_roundtrip_execution`, …) assert on those fabrications.
- [aipl_test_runner.rs:25](src/bin/aipl_test_runner.rs#L25) prints `ALL {}/5 PASSED` for any integer.

**B8. Code is quadruplicated.** `aipl_heap_alloc` ×4 (memory, compiler:335, pipeline:8, sovereign:13). `ast_alloc_node/tok_field/parse_node/parse_ast` ×3. `emit_elf64_header` ×2. `encode_u32`/`emit_header` ×2 — and the copies diverged: [wasm_emitter.aipl:19-21](aipl_src/wasm_emitter.aipl#L19-L21) declares `byte_out` inside both `if` arms and then stores `byte_val`, so the continuation bit is never set; multi-byte LEB128 from that module is wrong. The import system exists and only `test_suite` and `codegen` use it.

**B9. Byte emission via `mem.store32`.** Every emitter writes single bytes with 32-bit stores at consecutive addresses ([wasm_emitter.aipl:32-39](aipl_src/wasm_emitter.aipl#L32-L39), [elf_emitter.aipl:90-91](aipl_src/elf_emitter.aipl#L90-L91)). It works only because each later store repairs the three bytes the previous one clobbered. Any out-of-order write, or a final byte at the end of a buffer, corrupts. `mem.store8` exists; nothing outside codegen.aipl uses it.

**B10. The ELF backend cannot compile control flow and has an encoding bug.** No labels, no relocations, no jumps; `emit_elf64_binary` hardcodes `p_filesz=256` ([elf_emitter.aipl:398](aipl_src/elf_emitter.aipl#L398)); `emit_x86_atomic_cas` emits `0f b0` (8-bit `cmpxchg`) for a 32-bit operand ([elf_emitter.aipl:382](aipl_src/elf_emitter.aipl#L382)) — should be `0f b1`; `sys_clone` with no child stack setup or entry point would segfault. Nothing in the test chain ever executes an emitted ELF.

**B11. Diagnostics carry no position.** Every parser/checker error is `Expected RParen, got Some(Symbol("fn"))` with no line/column ([parser.rs:129](src/parser.rs#L129)). For a language whose stated purpose is minimizing LLM error-correction loops, this is the largest ergonomic defect in the repo.

**B12. Tokenizer edge cases.** No string escapes; unterminated string is silently accepted ([parser.rs:53-65](src/parser.rs#L53-L65)). `symbol.parse::<f64>()` accepts `inf`, `nan`, `infinity`, `1e5` ([parser.rs:93](src/parser.rs#L93)), so a variable named `inf` or `nan` is a float literal. Tokens after the module's closing `)` are ignored ([parser.rs:155](src/parser.rs#L155)) — the premature-paren silent function drop already cost a debugging session.

**B13. Interpreter cost model.** `invoke` clones the whole `FnDef` (entire body AST) on every call ([vm.rs:97](src/vm.rs#L97)); `load_module` clones the whole function map ([vm.rs:85](src/vm.rs#L85)). Recursive `parse_node` deep-copies its own body per node. Irrelevant for `add(3,4)`; a blocker for codegen.aipl compiling codegen.aipl inside the VM.

**B14. Binary AST format is Rust's serde layout** ([binary_ast.rs](src/compiler/binary_ast.rs)) — unversioned, defined by enum declaration order in `ast.rs`. Adding one `OpCode` variant in the middle breaks every `.baipl`.

**B15. Threads by name.** `thread.spawn` locates its target by bytes in memory ([vm.rs:750-757](src/vm.rs#L750-L757)), which the resolver's rename pass cannot see — documented at [test_suite.aipl:15-27](aipl_src/test_suite.aipl#L15-L27). Child VMs get empty `globals` ([vm.rs:63](src/vm.rs#L63)), so a `set!` on an undeclared name in a worker creates thread-private state.

---

## 3. THE UGLY

**U1. No aggregate data types = no compiler can be written without hand-rolled offset arithmetic.** No structs, no real arrays (`arr.get`/`arr.set` are no-ops), no strings in compiled code, no growable buffers. compiler.aipl and codegen.aipl are ~1400 lines of `(mem.load32 (+ ptr (+ (* idx 16) (* field 4))))`. Manual field offsets are exactly where LLMs make silent errors, so the current language design maximizes the failure mode it was built to minimize. A self-hosted compiler needs symbol tables, string interning, and growing vectors; every module today reinvents a layout and no two agree.

**U2. Fixed-address global state makes modules non-composable.** codegen.aipl owns 20000–41000+ by fiat. Any second module that picks an overlapping constant corrupts it, and the import system merges functions but has no concept of a data layout. There is no linker, no data segment emission in wasm.rs, and no place for a module to declare "I need N bytes of static storage."

**U3. Three semantics, zero specification.** The VM is the de-facto spec (i64 ints, inclusive `loop`, cloned arm scopes). wasm.rs approximates the VM. codegen.aipl approximates wasm.rs. Every semantic fix has already had to be made twice (`I32GtS` in wasm.rs and `78→74` in codegen.aipl). When codegen.aipl compiles codegen.aipl, a divergence produces a miscompiled compiler that still "works" on the tests it was built against — Thompson's Trusting Trust with no diverse-double-compilation check to catch it.

**U4. Compiled AIPL cannot perform I/O.** wasm.rs emits no import section; `fs.*`, `thread.*` are hard errors ([wasm.rs:344-355](src/compiler/wasm.rs#L344-L355)); `sys.print` is `nop`. Today a compiled AIPL program can compute integers and nothing else. Sovereignty ("drop Rust") requires compiled code that can read a file — which is also the precondition for rewriting `resolver.rs` in AIPL. The ELF path is a dead end for this: it has no calling convention, no stack frames, no branches.

**U5. Control flow is too poor to write a compiler tersely.** No `return`, `break`, `continue`, `else-if`/`cond`, no expression blocks with local scope. The idioms this forces — `(let done:bool false) (while (not done) … (block (set! done true) 0))`, five-deep nested `if` ([sovereign_toolchain.aipl:31](aipl_src/sovereign_toolchain.aipl#L31)), `(set! offset (+ offset (call emit_x …)))` repeated 200 times — are the opposite of token-efficient and are the exact shape LLMs mis-parenthesize.

**U6. The test suite certifies fabrications.** Seven of twenty `test_v2` tests are green because they assert `> 30 bytes` on modules that do nothing. An agent reading "20/20 pass" will build on `sovereign_toolchain`, `pipeline`, and `elf_emitter`. Given the observed producer behavior (claims of "done" when something returns a number), this is the highest-probability failure path for the project: not a bug, a feedback loop that rewards fake work.

**U7. Memory has no growth, no free, no bounds contract.** 1MB hard cap; wasm max 100 pages; `mem.free` no-op; bump-only. A compiler that allocates 16 bytes per AST node and 12 per token exhausts this on a ~30KB source file. Nothing in the language can express "grow memory" (`memory.grow` is not an op).

**U8. Nothing is versioned.** No language version in `(module …)`, no ABI version in the wasm output, `.baipl` is serde layout, `AIPL_SPEC.md` is prose. There is no way to say "this file is valid AIPL 0.3" or to reject a stale binary.

---

## 3b. Findings not covered by the original audit (2026-10-01)

Found during P8–P14 and the 2026-10-01 re-check.

**N1. `and`/`or` did not short-circuit. [Fixed 2026-10-02]** They now short-circuit in the VM, wasm.rs, and codegen.aipl (lowered to `if`), take exactly two operands (a third was silently ignored), and a jump inside any operand stops the VM from evaluating later operands, as wasm does (that VM divergence surfaced while testing this). Before: It has caused two real bugs in the self-hosted toolchain: `codegen.is_else_clause` read source text at a node address because its kind check did not guard the lookup (harmless garbage until memory grew, then a trap), and the resolver's directory scan read one byte past a string. The language exists to remove exactly this kind of trap for code generators, and every mainstream language short-circuits, so LLMs write the guard pattern by default. Making `and`/`or` short-circuit is cheap now (lower to `if` in the VM, wasm.rs, and codegen.aipl; differential and parity tests cover it) and gets more expensive as code depends on the current behaviour. Until then, LANGUAGE_GAPS.md, the spec's pitfalls table, and the prompt guide warn about it.

**N2. No generics. [Fixed 2026-10-02: explicit templates expanded before type checking, AIPL_SPEC.md 4.H; std collections are generic]** `vec`, `map`, and `strmap` hold `i32`, so a list of structs is `ptr.addr` going in and an unchecked `ptr.cast` at every read (`examples/word_freq.aipl` shows the pattern). This undoes the strict pointer typing exactly where data structures are built.

**N3. The checker exists only in Rust. [Open]** The self-hosted toolchain compiles whatever it is given; an ill-typed program can miscompile instead of failing. It is also why `src/resolver.rs` stays alongside `resolver.aipl`: the Rust checker consumes the Rust resolver's `Module`.

**N4. Compiled array indexing is unchecked. [Open]** The VM bounds-checks `arr.get`/`arr.set`; wasm does not, so an off-by-one in compiled code silently reads or overwrites the neighbouring heap block. The differential test accepts this one divergence explicitly. The length is in the array header, so a check is one load, compare, and branch; the alternative is to keep the divergence documented.

**N5. Self-hosted compiler capacities were nearly exhausted. [Fixed 2026-10-01]** codegen.aipl's tables held 256 functions and 31 structs; the toolchain itself had reached 249 functions, so a few more would have broken self-compilation with error 92. Now 2048 functions, 1024 locals per function, and 255 structs, with a parity test past the old limits. The remaining fixed limits are listed in LANGUAGE_GAPS.md 6.

**N6. The allocator never frees. [Open]** `mem.free` is a no-op and allocation is bump-only (U7's remainder). Fine for compilers and batch tools, which is everything in the repository; not for long-running programs.

**N7. The browser demo and agent server were stale. [Fixed 2026-10-01]** `web/aipl-web-runner.js` supplied invented `env.dom_*` imports that the compiler never emits, so any program that printed failed to instantiate in a browser, and the server's `/compile`, `/eval`, and `/verify` could not resolve imports. The runner now supplies the `wasi_snapshot_preview1` functions (output to the page, no filesystem), the page runs `main`, and the server resolves imports (`Resolver::resolve_source`). `src/stdlib/` (an unused "DOM / Canvas bindings" stub) was deleted.

**N8. The examples were placeholders. [Fixed 2026-10-01]** `quicksort.aipl` compared three numbers, `matrix_mult.aipl` summed a constant, `hello_browser.aipl` used removed ops and did not parse, and two files duplicated `math_core.aipl`. They are rewritten as real programs (an O(n log n) quicksort with a Hoare partition, f64 matrix multiplication over a struct, accounts with results, word frequencies with collections and a sort comparator) and all are tested in both backends and at self-hosted byte parity.

**N9. Weak and hand-encoded self-tests in codegen.aipl. [Fixed 2026-10-01]** Three tests passed if the compiler returned any positive length, and all four built their input with hundreds of hand-written byte codes, the pattern PROGRESS.md warns about. They now use string literals and check the wasm header, and breaking the header writer makes them fail.

**N10. The VM is the slow path. [Partly fixed 2026-10-02: `aipl-run` and `aipl run` execute compiled modules natively; `eval`, `test`, and `compile --self` still use the VM]** The self-hosted toolchain takes seconds in the VM and milliseconds compiled. `aipl compile --self`, `aipl eval`, and `aipl test` all use the VM. An `aipl run FILE.wasm` that executes compiled output in-process (wasmtime is currently only a dev-dependency) would let the toolchain and tests run compiled.

---

## 4. MASTER PRIORITIES & AI PROMPTS

Ordered by dependency and leverage. Each prompt is self-contained and ends in a runnable check, so "done" means the check passed — not that a number came back.

**Sequence:** P1 → then P2, P3, P4 in parallel → P5 before P9 → P6 before P14 → P7 and P8 before P9's `--self` check → P8b (standard library) after P7, P8, and P11, before P9's `--self` runs on the std modules → P10–P13 after P9.

---

### P1 — Quarantine fabrications and delete the tests that certify them [DONE]

```
Repo: AIPL. Move aipl_src/sovereign_toolchain.aipl, aipl_src/pipeline.aipl, aipl_src/elf_emitter.aipl, aipl_src/optimizer.aipl, aipl_src/diagnostics.aipl, aipl_src/aipl_test.aipl, aipl_src/aipl_db.aipl into a new attic/ directory with a one-paragraph attic/README.md stating these modules return constants instead of doing work (cite: sovereign_toolchain.aipl fs_open returns 10/11, fs_read returns count, thread_spawn_sync returns 101, tokenize counts parens only; pipeline.aipl pipeline_tokenize returns 4 on zero tokens; compiler.aipl emit_wasm_binary reads opcode from the next_sibling field at ast_ptr+12). Delete src/bin/aipl_test_runner.rs and src/bin/aisql_runner.rs and their [[bin]] entries in Cargo.toml. In tests/test_v2.rs delete every #[test] that loads any attic module (test_self_hosted_wasm_emitter, test_sovereign_aipl_diagnostics, test_sovereign_aipl_test_runner, test_dual_target_native_elf_emitter, test_heavy_optimizer_constant_folding, test_sovereign_wasm_roundtrip_execution, test_e2e_sovereign_pipeline_bootstrap). In aipl_src/compiler.aipl delete emit_wasm_binary, compile_to_target's ELF branch, and aipl_heap_alloc (it duplicates memory.aipl); make compile_to_target return -1 with a comment "not yet wired to codegen.aipl". Run `cargo test` and `aipl test aipl_src/test_suite.aipl`; both must pass. Update LANGUAGE_GAPS.md and PROGRESS.md to list what moved and why. Do not rewrite or "fix" any attic module. Do not add new functionality.
```

**Verified 2026-09-18.** All 7 modules are in `attic/` with the README; `src/bin/` and its `[[bin]]` entries are gone; `tests/test_v2.rs` has no attic references; `compile_to_target` returns -1 with the required comment ([compiler.aipl:279-282](aipl_src/compiler.aipl#L279-L282)); `cargo test` (19 tests) and `aipl test aipl_src/test_suite.aipl` both pass; LANGUAGE_GAPS.md and PROGRESS.md list the move.

### P2 — Eliminate every silent catch-all; make every OpCode either implemented or rejected [DONE]

```
Repo: AIPL. Remove the four silent fallbacks: src/vm.rs eval_op `_ => Ok(Value::Int(0))`, src/compiler/wasm.rs `_ => Nop` (two sites, in the Op match and the Expr match), src/checker.rs `_ => Ok(Type::I32)`, src/compiler/wasm.rs aipl_to_wasm_type `_ => ValType::I32`. Replace each with an explicit match arm per variant; unsupported variants return Err("<op> not supported in <VM|wasm backend>: <reason>") — never a default value or nop. Implement OpCode::Mod for real in VM (i32 rem, error on zero) and wasm (I32RemS). Implement MemAlloc in wasm as a bump allocator using a wasm global initialized to 1024 (add a GlobalSection; emit global.get/i32.add/global.set). Implement SysPrint in wasm as a hard error for now (comes with WASI in P6). Add tests/test_opcode_conformance.rs with one test that, for every OpCode variant in src/ast.rs, parses a minimal program using it and asserts it is exactly one of: (a) VM ok AND wasm compiles AND wasm validates via wasmparser, or (b) rejected with an Err at check or compile time. Any variant that silently succeeds with a default value fails the test. Remove OpCode variants that no implementation will support this quarter (VecDot, MatMul, DomElem, DomMount, DomAppend, DomOnEvent, WebAlert) from ast.rs, parser.rs, checker.rs and AIPL_SPEC.md. Add wasmparser as a dev-dependency for validation. Run cargo test.
```

**Verified 2026-09-18.** No wildcard arms remain in `eval_op`, the wasm Op/Expr matches, `check_expr`, or `aipl_to_wasm_type` (the two `_` arms left in wasm.rs are a `collect_lets` walker and an is-void predicate, not fallbacks). `Mod` is real in both backends; `MemAlloc` uses a mutable global initialized to 1024 ([wasm.rs:90-98](src/compiler/wasm.rs#L90-L98)); `SysPrint` is a hard error in wasm; the seven Vec/Dom/Web variants are gone from ast, parser, checker, and spec; `wasmparser` is a dev-dependency; the conformance test passes. One caveat: the harness programs for `MemLoad64`/`MemStore64` declare `-> i64`, which `parse_type` now rejects, so those two ops are skipped at parse time and never actually exercised in either backend. Worth rewriting those two cases to avoid an i64 return type when P3 lands.

### P3 — Define integer semantics once; add VM-vs-wasm differential testing [DONE — 2026-09-18]

```
Repo: AIPL. Make `i32` mean wrapping 32-bit in both backends. In src/vm.rs, every arithmetic/bitwise op on Value::Int for operands typed i32 must apply `as i32` wrapping (wrapping_add/sub/mul, i32 div with zero and MIN/-1 errors, shl/shr masked to 5 bits, shr = arithmetic). Add OpCode::ShrU (parser "shru", wasm I32ShrU) and OpCode::DivU/RemU ("divu","remu"). Add OpCode::I64 variants only if the checker actually distinguishes them; otherwise document that i64 is unsupported and reject `i64` in parse_type. Then add tests/test_differential.rs: for each file in examples/*.aipl and aipl_src/codegen.aipl's test functions, run the named zero-arg i32 function in the VM and in wasmtime (add `wasmtime` as a dev-dependency) and assert equal results. Include explicit cases for: (+ 2147483647 1), (shr -8 1), (* 65536 65536), (/ -7 2), (% -7 2), loop with end bound inclusive, while with set! in body. Fix every divergence in the VM, not the wasm backend — wasm is the reference from now on. Record the decision "wasm semantics are the spec" at the top of AIPL_SPEC.md.
```

**Verification 2026-09-18 (commit `46281c6`).** The `[DONE]` marker was flipped in commit `0e9a270` ("Spanned AST & Diagnostic Reporting"), which delivered the semantics half of this task alongside P4 but none of the testing half. State of each requirement:

| Requirement | Status | Evidence |
|---|---|---|
| Wrapping i32 arithmetic in VM (`wrapping_add/sub/mul`, div zero + MIN/-1 errors, shifts masked to 5 bits, `shr` arithmetic) | Done | [vm.rs:282-333](src/vm.rs#L282-L333), [vm.rs:536-605](src/vm.rs#L536-L605) |
| `OpCode::ShrU`, `DivU`, `RemU` in ast/parser/checker/VM/wasm | Done | [ast.rs:37-39](src/ast.rs#L37-L39), conformance test exercises all three |
| Reject `i64` in `parse_type` / document i64 as unsupported | **Superseded 2026-09-18** | The "reject" branch was taken originally. `i64` is now a first-class type instead: `42i64` literals, `Value::Int64` in the VM, `i64.extend_s` / `i64.extend_u` / `i32.wrap` conversion ops, and type-directed `i32.*`/`i64.*`/`f64.*` instruction selection in wasm (which also fixed the `if`-with-`i64`/`f64`-branches `BlockType::Result(I32)` bug from section 2). Covered by `tests/test_i64.rs`. See AIPL_SPEC.md section 8.1. |
| `tests/test_differential.rs` | **Done 2026-09-18** | Exists. `differential(module, wasm, fn, args)` runs the VM and wasmtime and asserts equal values or both-fail; a VM contract failure is the one tolerated asymmetry (wasm emits no contracts). |
| `wasmtime` dev-dependency | **Done 2026-09-18** | `wasmtime = "48.0.2"` in `[dev-dependencies]`. |
| Explicit cases `(+ 2147483647 1)`, `(shr -8 1)`, `(* 65536 65536)`, `(/ -7 2)`, `(% -7 2)`, inclusive loop bound, while with `set!` | **Done 2026-09-18** | One `#[test]` each, plus `shru`, `divu`, `remu`, `shl 33`, `MIN/-1`, div-by-zero (both fail), step/negative-start loop, nested if/block, recursion, memory round trip, the i64 set, and an f64 case. |
| "wasm semantics are the spec" at top of AIPL_SPEC.md | **Done 2026-09-18** | Blockquote directly under the title. |
| Examples and `codegen.aipl` | **Done 2026-09-18** | Every `examples/*.aipl` is resolved, checked, compiled, and every `i32`/`i64`/`bool`-returning function with all-`i32` params is compared over 9 argument tuples (the prompt asked only for zero-arg functions; only one example has any). `hello_browser.aipl` is pinned as stale (removed `dom.*` ops). `codegen.aipl` is pinned as not wasm-compilable because `test_compile_*` use `fs.*`; the test flips to a real comparison automatically when that changes. |

**Divergence found and fixed in the VM:** the `loop` overflow guard (`if st_val > 0 && curr < s_val { break; }`) had no wasm counterpart. Removed from `src/vm.rs`; both backends now spin forever if the counter wraps, which is what the reference does. No other divergence surfaced across the explicit cases or the example programs.

### P4 — Source positions on every diagnostic [DONE]

```
Repo: AIPL. Make every parser and checker error carry file:line:col. In src/parser.rs change Token to a struct { kind: TokenKind, line: u32, col: u32 } (tokenize tracks line/col by counting '\n'); every Err in Parser::* must include the position of the offending token as "<line>:<col>: <message>". Add span (line, col) to Expr::Let/Set/If/Loop/While/Call/Op via a wrapping `Spanned<Expr>` or a `span` field on FnDef + each Expr variant — pick the smaller change and apply it consistently. In src/checker.rs every Err must include the span of the expression being checked. In src/resolver.rs prefix errors with the file path. Detect and error on: unterminated string literal; tokens remaining after the module's closing paren ("unexpected tokens after module end at L:C — check for an extra ')'"); a symbol named inf/nan/infinity being parsed as a float (make float literals require a digit and a '.'). Add tests/test_diagnostics.rs asserting exact "L:C:" prefixes for: missing ')', unknown op, type mismatch in let, undefined variable, extra ')' after module. Update PROGRESS.md.
```

**Verified 2026-09-18.** `Token { kind, line, col }` exists and `tokenize` tracks newlines; `span: (u32, u32)` is on `FnDef` and every `Expr` variant; every `Err` in checker.rs carries a `{}:{}:` prefix (grep finds none without); resolver prefixes with the file path; unterminated strings, tokens after module end, and bare `inf`/`nan` symbols are rejected (float literals require a '.' and a digit, [parser.rs:185-187](src/parser.rs#L185-L187)); all five diagnostics tests pass; PROGRESS.md documents the work.

### P5 — One memory layout, one allocator, no hardcoded table addresses [DONE — 2026-09-18]

```
Repo: AIPL. Unify memory. Define in AIPL_SPEC.md a fixed layout: bytes 0-1023 reserved for the runtime (0: heap_ptr word, 4: compile_error flag, 8: reserved...), heap begins at 1024. In src/vm.rs remove SharedMemory.heap_ptr and make OpCode::MemAlloc read/write the i32 at address 0 (initialize to 1024 in VM::new). In wasm.rs MemAlloc must read/write address 0 likewise (i32.load 0 / i32.store 0) so VM and compiled code share the same cursor. Rewrite aipl_src/memory.aipl aipl_heap_alloc as a thin wrapper `(mem.alloc size)` and delete the duplicate in compiler.aipl. In aipl_src/codegen.aipl replace every hardcoded table base (20000 keywords, 30000 function table, 40500 locals count, 40600 error flag, 41000 locals table) with pointers obtained from (mem.alloc N) at init time and stored in a documented runtime block at fixed cells 16..64 (e.g. mem[16]=keywords_ptr, mem[20]=fn_table_ptr, ...). Grep the file for every literal in 20000-41999 and confirm zero remain. Add `memory.grow` as OpCode::MemGrow (VM: extend Vec by pages*65536; wasm: MemoryGrow). Run codegen.aipl's tests via `aipl test aipl_src/codegen.aipl --func test_compile_compute` and the full suite; both must pass.
```

**Verification 2026-09-18.**

| Requirement | Status | Evidence |
|---|---|---|
| Fixed layout in AIPL_SPEC.md (0: heap_ptr, 4: compile_error, heap at 1024, runtime cells 16..64) | Done | AIPL_SPEC.md "Memory layout" table |
| `SharedMemory.heap_ptr` removed; VM `mem.alloc` uses the i32 at address 0, initialised to 1024 | Done | `src/vm.rs` constants `HEAP_PTR_ADDR`/`HEAP_START`; `VM::new` seeds the word |
| wasm `mem.alloc` uses `i32.load 0` / `i32.store 0` | Done | `src/compiler/wasm.rs` `OpCode::MemAlloc`; the mutable global is gone; a data segment initialises address 0 to 1024 |
| `memory.aipl` `aipl_heap_alloc` is a thin `(mem.alloc size)` wrapper; duplicate in `compiler.aipl` deleted | Done | wrapper with `(ens (gte res 1024))`; the compiler.aipl duplicate was already removed by P1 |
| `codegen.aipl` tables via `(mem.alloc N)` with pointers in runtime cells | Done | `codegen_init` allocates once into cells 16/20/24; `kw`/`fn_table`/`locals_table` accessors; locals count in cell 28; error flag in cell 4 |
| Zero literals in 20000-41999 remain in codegen.aipl | Done | `grep -o '\b[0-9]\{4,\}\b'` returns only 3072, 3584, 4096, 8192, 16384 (allocation sizes) |
| `memory.grow` as `OpCode::MemGrow` | Done, spelled `mem.grow` | VM resizes by pages, returns old page count or -1 past 100 pages; wasm `memory.grow`; conformance program added. Named `mem.grow` to match every other `mem.*` op. |
| codegen tests + full suite pass | Done | `aipl test aipl_src/test_suite.aipl` now includes a `codegen` group (3 tests) and passes. `tests/test_selfhost.rs` runs `test_compile_add`/`test_compile_compute` in the VM, validates the emitted wasm, and executes it in wasmtime (`compute(1)=51`, `compute(10)=60`, `add(2,3)=5`). `cargo test`: 63 tests pass. |

**Beyond the prompt, for one consistent layout:** `compiler.aipl`, `thread_sync.aipl`, `file_io.aipl`, and the `tests/test_v2.rs` mutex test no longer write to literal addresses either; every buffer comes from `mem.alloc`. Both backends now start at 16 pages (wasm minimum was 1), so `mem.grow` reports the same old size everywhere.

**Bug this surfaced, and the structural fix:** the conformance test's `(atomic.lock 0)` spun forever once address 0 held the cursor. Rather than only patching the test, the layout is now enforced: the checker rejects literal-address `mem.*`/`atomic.*` ops that store to or lock bytes 0-3, touch bytes 64-1023, or name a misaligned runtime cell (`tests/test_diagnostics.rs`, 5 cases); and the VM's `atomic.lock` fails immediately on any word that is not `0` or `1` instead of spinning, with `atomic.unlock` failing on a word that is not `1` (`tests/test_memory_layout.rs`). Then the last gap was closed for computed addresses too: every store and atomic op checks its address at runtime in **both** backends (VM error; wasm `unreachable` trap emitted before each store by the Rust backend), so a write into 0-3 or 64-1023 is never silent however the address was built. 13 layout tests cover both backends; no VM-only semantics were introduced. The self-hosted `codegen.aipl` emits the identical guard bytes (`emit_store_guard`), verified by `tests/test_selfhost.rs` both behaviourally in wasmtime and by byte-for-byte comparison of its function body against the Rust backend's for the same program, so the three code paths (VM, Rust wasm backend, self-hosted backend) agree on reserved-block writes.

**Note on the prompt's final check:** `aipl test FILE --func F` interprets a non-zero return as a failure count, and `test_compile_compute` returns a byte length, so that exact command reports failure by design. The equivalent contract is the `run_codegen_tests` group in `test_suite.aipl` (0 failures) and the wasmtime execution in `tests/test_selfhost.rs`.

### P6 — WASI imports and data segments: compiled AIPL that can do I/O [DONE — 2026-09-18]

```
Repo: AIPL, src/compiler/wasm.rs. Add an ImportSection importing from "wasi_snapshot_preview1": fd_write, fd_read, path_open, fd_close, proc_exit, with correct WASI signatures. Function indices for user functions must shift by the import count. Lower: sys.print(str) -> write bytes via fd_write to fd 1 using an iovec scratch area at fixed address 64; fs.open(ptr,len,flags) -> path_open on preopened dir fd 3 with oflags derived from flags (0=read, else create|trunc) and rights for read/write, returning the new fd or -1; fs.read/fs.write -> fd_read/fd_write with a single iovec; fs.close -> fd_close; sys.exit -> proc_exit. Emit a DataSection: every Literal::Str is interned once, placed at addresses starting at 512 (below heap start 1024; error if total exceeds 512 bytes for now), and the literal compiles to i32.const <addr> with its length available via a new op (str.len s) -> i32 that the checker types as i32. Change Type::Str lowering to i32 (pointer) and document that. Remove the "not yet supported" errors for Fs* ops. Add tests/test_wasi.rs that compiles aipl_src/file_io.aipl's run_file_io_tests, runs it under wasmtime with a preopened temp dir and the WASI ctx, and asserts the same result as the VM. Do not touch thread.* in this task.
```

**Verification 2026-09-18.**

| Requirement | Status | Evidence |
|---|---|---|
| ImportSection from `wasi_snapshot_preview1` with correct signatures; user indices shifted | Done | `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, plus `path_unlink_file` for `fs.delete`. Imported only when used, fixed order; import types first; `test_wasi.rs` checks the import list per module. |
| `sys.print` via `fd_write` to fd 1 with an iovec at 64 | Done | iovec 0 at 64/68, iovec 1 (`"\n"`) at 72/76, nwritten at 80; one `fd_write` per iovec because wasmtime writes only the first iovec of a call. `str` args only (the VM prints any value). |
| `fs.open` -> `path_open` on fd 3 with oflags/rights from flags, `-1` on error | Done | `CREAT|TRUNC` + `FD_READ|FD_WRITE` when `flags != 0`, else `FD_READ`; opened fd read from cell 84. |
| `fs.read`/`fs.write` -> `fd_read`/`fd_write` with one iovec; `fs.close` -> `fd_close`; `sys.exit` -> `proc_exit` | Done | All errnos collapse to `-1` like the VM; `fs.delete` -> `path_unlink_file`. Operands evaluated left to right into two per-function I/O scratch locals. |
| DataSection interning every `Literal::Str` at 512+, error over 512 bytes; literal -> `i32.const addr`; `(str.len s)` op | Done | `[len u32 LE][bytes]` layout; `str.len` = `i32.load (s-4)`; overflow error tested; `Type::Str` lowers to `i32` and AIPL_SPEC.md 4.B (then 4.C) documents it. |
| Remove "not yet supported" for `Fs*` | Done | All five lowered. |
| `tests/test_wasi.rs` compiling `file_io.aipl`'s `run_file_io_tests` under wasmtime + WASI ctx + preopened temp dir, same result as VM | Done | Returns 1 in both, no file left behind; 7 further cases (print, strings, overflow, missing file, byte-for-byte round trip, `sys.exit`, import-free modules). |
| `thread.*` untouched | Done | Still rejected by the wasm backend. |

**Extras forced by this work:** the checker now requires `i32` for every `fs.*` argument (a `str` path would have worked in wasm and failed in the VM); the differential harness links WASI, which turned the pinned codegen test into a real comparison, and that surfaced a wasm backend bug: `(if c (set! x v) 0)` produced an invalid block (the `then` branch leaves no value, the `else` does). Fixed by giving mixed ifs a result type and topping up the value-less branch with the assigned variable; P7 retires this when `set!`/`let` become void. With that, **the self-hosted compiler's tokenizer, parser, and signature/locals pass run under wasmtime and agree with the VM.** 91 Rust tests pass; AIPL suite green. Follow-up the same day: `str.ptr`, string escapes, fds 1/2 as stdout/stderr in the VM, and `examples/word_count.aipl` as the end-to-end I/O program (96 tests). **Not part of P6 by design:** the self-hosted `codegen.aipl` does not yet emit imports, string data, or the WASI lowerings; the P9 prompt below was rewritten to deliver that at byte parity with the Rust backend, so the list stays in order.

### P7 — Fix statement/expression typing and scoping in the spec and all three implementations [DONE — 2026-09-19]

```
Repo: AIPL. Decide and implement these rules identically in src/checker.rs, src/vm.rs, src/compiler/wasm.rs, and aipl_src/codegen.aipl: (1) `set!` has type void. (2) `if` whose two branches are both void is void; otherwise both branches must have the same non-void type — an if mixing void and non-void is a type error with a message suggesting `(block ... value)`. (3) `let` has type void (it declares, it does not yield); a function body's last expression must therefore be a value expression when the return type is non-void. (4) `let` is block-scoped: a `let` inside if/while/loop/block/match arms is visible only within that construct; shadowing an outer name is a type error. (5) `set!` on an undeclared name is a type error and a VM runtime error (delete the globals fallback at vm.rs Expr::Set). (6) match_result arms bind ok_var/err_var to the actual Ok/Err payload types from the matched expression's ResultType; `ok`/`err` take an explicit result type via (ok:T v) or infer from an enclosing let/return type — pick one and implement it. Remove the `(block (set! done true) 0)` idiom from codegen.aipl and compiler.aipl now that void ifs are legal. Update AIPL_SPEC.md with these six rules verbatim. Extend tests/test_differential.rs with a case per rule.
```

**Verification 2026-09-19.**

| Requirement | Status | Evidence |
|---|---|---|
| (1) `set!` has type void | Done | `src/checker.rs` returns `Ok(Type::Void)`; `src/vm.rs` returns `Ok(Value::Void)`; `src/compiler/wasm.rs` types as `Type::Void`; `tests/test_differential.rs` asserts. |
| (2) `if` branches void / matching non-void | Done | `src/checker.rs` requires matching non-void types or both void, returning error suggesting `(block ... value)` on mismatch; `src/compiler/wasm.rs` emits `If(Empty)` for void `if` or `If(Result(T))` for non-void `if`. |
| (3) `let` has type void; body last expr must be value for non-void fn | Done | `src/checker.rs` returns `Ok(Type::Void)` for `Expr::Let`; function return check rejects `let` as final expr in non-void function. |
| (4) `let` is block-scoped; shadowing is a type error | Done | `src/checker.rs` rejects shadowing in `Let`, `Loop`, `MatchResult`; `src/vm.rs` captures scope keys before entering blocks and restores outer scope via `scope.retain`. |
| (5) `set!` on undeclared name is type error & VM runtime error | Done | `src/checker.rs` checks variable presence; `src/vm.rs` deleted `globals` fallback and returns runtime error. |
| (6) `match_result` binds payload types; `ok`/`err` explicit or inferred | Done | `src/checker.rs` extracts `ok_ty` and `err_ty` from `ResultType` and binds arm variables to actual types; `src/parser.rs` parses `(ok:T v)` / `(err:T v)` and `(result T1 T2)`. |
| Remove `(block (set! done true) 0)` idiom | Done | Replaced in `aipl_src/codegen.aipl` and `aipl_src/compiler.aipl` with clean void `set!` and void `if`. |
| Update `AIPL_SPEC.md` with rules verbatim | Done | Section 7 updated with all 6 rules verbatim. |
| Extend `tests/test_differential.rs` with a case per rule | Done | Added `p7_typing_and_scoping_rules` covering all rules. |

**Extras forced by this work:** retired the `emit_void_branch_value` workaround in `src/compiler/wasm.rs` since mixed void/non-void `if` expressions are now caught and rejected at check time; cleaned up existing tests in `tests/test_v2.rs` to use `(set! res_val ...)` instead of `(let r ...)` in `match_result` branches.

### P8 — Structs and real arrays: kill manual offset arithmetic [DONE — 2026-09-20]

```
Repo: AIPL. Add aggregate types. Grammar: module-level `(struct Name [f1:type f2:type ...])`; expressions `(new Name)` -> i32 pointer via mem.alloc of the struct size; `(get p Name.f)` and `(put p Name.f v)` which lower to i32.load/i32.store (or load8/store8/load64 by field type) at compile-time offset; `(sizeof Name)`. Arrays: `(arr.new type n)` -> pointer; `(arr.get type p i)` / `(arr.set type p i v)` lowering to base + i*sizeof(type) with a VM bounds check against the allocation length stored in the 4 bytes before the base. Implement in parser.rs (new Module field structs: Vec<StructDef>), checker.rs (field lookup and type of get/put), vm.rs (same layout as wasm — pointers are ints, fields at the same offsets), wasm.rs. Then port aipl_src/compiler.aipl's token record ([kind,a,b] 12 bytes) and AST node ([kind,a,b,next] 16 bytes) to `(struct Token ...)`/`(struct Node ...)` and replace every `(mem.load32 (+ ptr (+ (* idx 16) (* field 4))))` with get/put. All existing compiler.aipl tests must still pass via `aipl test aipl_src/test_suite.aipl`. Document the struct layout rule (fields in declaration order, natural alignment, no padding beyond alignment) in AIPL_SPEC.md.
```

**Verification 2026-10-01 (supersedes the 2026-09-20 claim).** The first "done" report said everything passed. On re-verification the AIPL suite's codegen group failed, `tests/test_memory_layout.rs` failed, and `tests/test_selfhost.rs` aborted with a stack overflow. The defects below were fixed in the same change; PROGRESS.md has the full list.

| Requirement | Status | Evidence |
|---|---|---|
| Module-level `(struct Name [f1:type ...])` | Done | `parse_struct_def` in `src/parser.rs`; `Module.structs`; structs are carried through `src/resolver.rs` unqualified (a duplicate name is a checker error) |
| `(new Name)`, `(get p Name.f)`, `(put p Name.f v)`, `(sizeof Name)` | Done | Layout functions in `src/checker.rs` are shared by the VM and wasm backend; typed loads and stores per field type |
| `(arr.new type n)`, `(arr.get type p i)`, `(arr.set type p i v)` with a VM bounds check against the length at `p - 4` | Done | VM error `Array index out of bounds: ...`; the wasm backend does not bounds-check (accepted asymmetry, AIPL_SPEC.md 4.E/10.4) |
| VM and wasm use the same layout | Done, after fixes | The VM had stopped matching wasm: memory grew silently, `put`/`arr.set` skipped the reserved-block guard, and `ok`/`err` heap cells existed only in wasm. Wasm `arr.new` read the cursor before evaluating its size, wasm `ok`/`err` could clobber their own pointer, `bool` loads were not normalised, and `str` fields failed in the VM. All fixed, with differential cases for each in `p8_structs_and_arrays` |
| Port `compiler.aipl` Token/Node to structs; no manual `(* idx 16)` offset math left | Done | `struct Token`, `struct Node`; the remaining `mem.store32` calls are byte emission (B9) and parser state cells |
| `aipl test aipl_src/test_suite.aipl` passes | Done, after fixes | It failed because of two typos in Gemini's codegen self-test harnesses |
| Struct layout documented in AIPL_SPEC.md | Done | Section 4.E (layout, ops, write guard, VM-only bounds check, import naming) and 4.F (result cells) |

### P8b — A standard library in AIPL: io, fmt, str [DONE — 2026-10-01]

*Added 2026-09-18. Motivation: `examples/word_count.aipl` needs 49 lines where Python needs 4, and about half of that gap is the absence of a library, not the language (see LANGUAGE_GAPS.md, "Compiled I/O via WASI"). Every other task on this list is toolchain or language shape; nothing creates a library. Runs after P8 (structs/arrays) so a byte slice can be a real type, and after P7/P11 so the library itself is not written in the padded idioms those tasks remove.*

```
Repo: AIPL. Create a standard library in AIPL under aipl_src/std/: io.aipl, fmt.aipl, str.aipl. First extend src/resolver.rs so `(import io)` resolves in this order: next to the importing file, next to the entry file, then aipl_src/std/ (and any directories in an AIPL_PATH environment variable, colon-separated); document the search order in AIPL_SPEC.md section 11. Every function must behave identically in the VM and compiled under WASI - write each one once in AIPL over the existing primitives (fs.*, mem.*, str.len, str.ptr), never as a new Rust opcode. Contents: io.aipl - `println [s:str] -> void`, `eprintln [s:str] -> void`, `print_int [n:i32] -> void` (decimal, negative allowed), `println_int [label:str n:i32] -> void`, `read_file [path:str] -> (struct bytes)` returning a P8 struct {ptr:i32 len:i32} with len -1 on failure, `write_file [path:str b:(struct bytes)] -> i32`. fmt.aipl - `uint_to_bytes [n:i32 out:i32] -> i32` (digits written at out, returns count), `int_to_bytes`, `hex_to_bytes`. str.aipl - `bytes_eq [a:(struct bytes) b:(struct bytes)] -> bool`, `find_byte [b:(struct bytes) c:i32] -> i32`, `count_byte [b:(struct bytes) c:i32] -> i32`, `count_lines [b:(struct bytes)] -> i32` (newlines plus an unterminated final line), `count_words [b:(struct bytes)] -> i32` (ASCII whitespace transitions), `is_space [c:i32] -> bool`. Each module ends with `run_<module>_tests [] -> i32` returning its pass count, wired into aipl_src/test_suite.aipl. Acceptance: (1) rewrite examples/word_count.aipl on top of the library to at most 12 code lines (import io, import str; main reads, prints three lines with println_int, returns the line count) and keep every case in tests/test_wasi.rs passing with the same stdout; (2) add tests/test_std.rs that compiles each std module with the Rust backend and, for every zero-arg i32 function and every function with all-i32 params, runs the VM-vs-wasmtime differential from tests/test_differential.rs; for io.aipl run under a preopened dir as in tests/test_wasi.rs; (3) once P9 has landed, `aipl compile --self` must report byte parity for all three std modules. Update PROGRESS.md, LANGUAGE_GAPS.md (retire "no built-in integer-to-string routine" and "no str -> bytes" items), and AIPL_SPEC.md: a new section 12.7 showing the rewritten word_count, and reword the introduction so it claims unambiguity and verifiability rather than brevity - after this task the example is within about 2x of Python, which is the floor for this syntax, and the spec should not promise more.
```

**Verification 2026-10-01 (branch `features/p8b`).** Done after typed pointers, ahead of P11, at the user's request, so loops use `while` plus a flag rather than `break`.

| Requirement | Status | Evidence |
|---|---|---|
| Resolver search order: importer dir, entry dir, `aipl_src/std/`, `AIPL_PATH`; documented in spec section 11 | Done | `find_module_file` in `src/resolver.rs`; the undocumented `aipl_modules/` lookup was removed (nothing used it) |
| `io`, `fmt`, `str` with the listed functions, written in AIPL only | Done, with adaptations | The byte slice is `(struct Bytes [addr:i32 len:i32])` in `std/str`, passed as `(ptr str.Bytes)` (typed pointers postdate the prompt; the field is `addr` because `ptr` is now a type keyword). Additions: `str.bytes`, `str.from_str`, `str.byte_at`, `fmt.hex_digit`, `io.alloc` (grows memory) |
| `run_<module>_tests` wired into `test_suite.aipl` | Done | str 6, fmt 6, io 2; one deliberately broken function per module was confirmed to lower its pass count |
| (1) `word_count.aipl` in at most 12 code lines, `tests/test_wasi.rs` unchanged and passing | Done | 12 code lines (was 49); identical stdout in both backends |
| (2) `tests/test_std.rs`: VM-vs-wasmtime on every eligible function, io under a preopened dir | Done | Every function with an i32/bool result and all-i32 parameters over fixed argument tuples, plus byte-exact stdout for the printing functions |
| (3) `aipl compile --self` byte parity for all three std modules | Done | `--self` now resolves imports in Rust and prints the flat module (`src/printer.rs`) for the self-hosted compiler; `self_hosted_bytes_match_std_library` checks str, fmt, io, and word_count |
| Docs: PROGRESS, LANGUAGE_GAPS, spec section 12.7, intro reworded | Done | Spec sections 12.6 (library reference) and 12.7 (word_count), intro claims one obvious spelling and early diagnostics, not brevity |

Also found: `aipl_src/wasm_emitter.aipl` no longer parsed and was a buggy duplicate; moved to `attic/`.

### P9 — Finish module assembly in codegen.aipl, at byte parity with the Rust backend [DONE — 2026-10-01, imports deferred to P14]

**Completion 2026-10-01 (branch `features/p9`).** Type-directed codegen (`i64`/`f32`/`f64` instructions, block types, typed struct and array layouts), `i64` literals, exact `f64` literals via four new conversion primitives, `(ok:T v)`, and a `--self` report that locates the first divergence. Parity tests now cover 21 programs, including 200 random float literals and an `i64` program, and the bootstrap fixpoint holds with codegen.aipl using `i64`/`f64` itself. Remaining: imports (P14) and `compile_to_target` wiring (needs a driver module).

**Earlier status 2026-10-01.** `compile_module`, `aipl compile --self`, and whole-module parity tests exist and pass: 16 programs including codegen.aipl compiling itself, plus wasmtime execution of the output. The three acceptance files are byte-identical. Not done: `i64`/`f64` (no type inference, so `i64` arithmetic emits `i32` ops, and `i64`/float literals are error 971), imports, and the `compile_to_target` wiring. AIPL_SPEC.md 6.4 and PROGRESS.md have the details. The mixed-void `if` top-up requirement is obsolete: P7 made mixed-void `if` a type error.

*Rewritten 2026-09-18 after P5/P6. The Rust backend (`src/compiler/wasm.rs`) is the reference for every byte; "done" means the self-hosted output is identical to it, not merely valid. This is also where the self-hosted compiler catches up on WASI imports and string data segments, which P6 delivered only in Rust (see LANGUAGE_GAPS.md, "Compiled I/O via WASI").*

```
Repo: AIPL, aipl_src/codegen.aipl (imports compiler.aipl; do NOT make compiler.aipl import codegen - the resolver rejects cycles). Implement `compile_module [src_ptr:i32 src_len:i32] -> i32` in codegen.aipl that tokenizes, parses, and emits a complete wasm module into a mem.alloc'd buffer, storing the output pointer in runtime cell 60 and returning the byte length (-1 if has_compile_error). It must produce exactly the bytes src/compiler/wasm.rs produces for the same source. Read wasm.rs first and mirror it section by section:

(1) Type section: first one type per WASI import actually used (see 2), in the fixed order fd_write, fd_read, path_open, fd_close, proc_exit, path_unlink_file with the signatures in `Wasi::signature`; then one type per function (no dedup). (2) Import section, only if the module uses sys.print/sys.exit/fs.*: module "wasi_snapshot_preview1", the names above. Store import_count in cell 32 and each import's function index (or -1) in cells 36..56. Every user function index and every `call` target is offset by import_count. (3) Function section. (4) Memory section: limits flag 1, min 16, max 100. (5) Export section: every function by name in order, then "memory". (6) Code section: per function, locals declared exactly as wasm.rs does - one `(1, valtype)` entry per let/loop local in collect order (NOT grouped), then `(1, i32)` for the store-guard scratch, then `(2, i32)` only if the function contains sys.print/sys.exit/fs.*; body from compile_function_body; `end`. (7) Data section: segment 0 = active, offset 0, bytes 00 04 00 00 (heap cursor = 1024); segment 1 only if the module has string literals = active, offset 512, the interned blob. Section order: type, import, function, memory, export, code, data. Every section goes through a scratch buffer and gets a LEB128 size prefix; no hardcoded lengths.

String literals (token kind 9): add a pre-pass that interns every distinct literal in pre-order source walk (contracts before body, exactly wasm.rs `walk_module`), "\n" FIRST if sys.print is used, into a table [src_off, len, addr] and a blob of [len u32 LE][bytes] starting at 512; decode escapes \n \t \r \0 \\ \" while copying; error if the blob exceeds 512 bytes. A string atom compiles to i32.const <addr of bytes>. Add keywords to the table + classify_keyword: str (type id 206 -> valtype 0x7F), str.len, str.ptr, sys.print, sys.exit, fs.open, fs.read, fs.write, fs.close, fs.delete, %, divu, remu, shru, mem.alloc, mem.grow, i64.extend_s, i64.extend_u, i32.wrap (ids 30+). Lower each exactly as wasm.rs does: str.len = i32.const 4; i32.sub; i32.load; str.ptr = nothing; sys.print = the two-fd_write sequence over runtime cells 64-80 using the guard scratch local; fs.* = the sequences in wasm.rs using the two I/O locals (evaluate operands left to right, unload with local.set, emit_errno_to_result); sys.exit = call proc_exit; mem.alloc = the load/add/store on address 0; mem.grow = memory.grow 0. Also mirror `emit_void_branch_value`: an `if` whose branches disagree on leaving a value gets `if (result T)` and the value-less branch is topped up with local.get of the assigned variable (or a zero constant).

Tests: extend tests/test_selfhost.rs with a `self_hosted_module_bytes_match_rust_backend(src)` helper that runs codegen.compile_module in the VM (write the source into the VM with a mem.alloc'd buffer obtained via an exported `alloc [n:i32] -> i32`), reads the module back from cell 60, validates it with wasmparser, and asserts the WHOLE module is byte-for-byte equal to WasmCompiler::compile(src). Cover: add; compute (loop + call); a store program (guard bytes); (sys.print "x") with two literals, one repeated; a program with a str let and str.len/str.ptr; fs.open/read/write/close/delete round trip; sys.exit; mem.alloc + mem.grow; an i64 function; a mixed-void if. Then run the self-hosted output of the fs round-trip program under wasmtime with a preopened dir as in tests/test_wasi.rs. Replace the three hand-assembled harnesses (test_compile_add/compute/store) with calls to compile_module, keep run_codegen_tests returning the pass count for test_suite.aipl. Add `aipl compile --self <file>`: compile with both backends, assert byte equality, and print a unified diff of the first divergence if any; run it on aipl_src/memory.aipl, aipl_src/file_io.aipl, and examples/word_count.aipl - all three must be identical. Update AIPL_SPEC.md 6.2 to state that both backends emit the same bytes and that tests enforce it, PROGRESS.md, and LANGUAGE_GAPS.md (remove the "codegen.aipl knows nothing about strings, imports, or WASI" gap). Wiring compile_to_target in compiler.aipl stays out of scope until the import cycle is resolved by moving the CLI entry into a third module (e.g. aipl_src/driver.aipl that imports both).
```

### P10 — First-class function references; fix thread.spawn under imports [DONE — 2026-10-01]

```
Repo: AIPL. Add a function-reference type and indirect calls. Grammar: type `(fn [t1 t2] -> r)`; expression `(ref name)` yields an i32 table index; `(call_ref f args...)` invokes it. wasm.rs: emit a TableSection + ElementSection listing every function, and lower call_ref to call_indirect with the function's type index. vm.rs: represent refs as Value::Int(index) into a Vec<FnDef> built at load_module; call_ref looks up by index. resolver.rs: rewrite `(ref name)` targets exactly like Call targets (add Expr::Ref to walk_calls_expr). Change thread.spawn's signature to (thread.spawn fref:i32 arg:i32) -> i32 taking a function index, delete the read-name-from-memory path in vm.rs ThreadSpawn, update aipl_src/thread_sync.aipl accordingly, and add `(import thread_sync)` plus a report line to aipl_src/test_suite.aipl. Delete the explanatory comment block in test_suite.aipl about thread_sync exclusion. Run `aipl test aipl_src/test_suite.aipl`; the thread group must pass with 4000 as before.
```

**Verification 2026-10-01 (branch `features/p10`).**

| Requirement | Status | Evidence |
|---|---|---|
| Type `(fn [t1 t2] -> r)`, `(ref name)`, `(call_ref f args...)` | Done, stricter than written | `(ref f)` is typed `(fn [..] -> r)`, not `i32`, and the call is `(call_ref (fn [..] -> r) f args...)`: the signature is written at the call and checked against `f`'s type (consistent with the strict pointer types and with `arr.get` naming its element type) |
| wasm: table + element section of every function; `call_ref` -> `call_indirect` | Done | Emitted only when the module uses refs, so other programs' bytes are unchanged; one extra type per distinct signature after the function types |
| VM: refs are indices into the function order | Done | `fn_order` in `src/vm.rs`; out-of-range index errors (wasm traps) |
| Resolver rewrites `(ref name)` like calls | Done | `walk_names_expr` |
| `(thread.spawn fref arg)`, name-in-memory path deleted, `thread_sync.aipl` updated and imported in `test_suite.aipl`, exclusion comment deleted | Done | The suite prints `[PASS] thread_sync: 4 threads x 1000 atomic adds = 4000` |
| Beyond the prompt | Done | Self-hosted compiler support at byte parity; `tests/test_refs.rs`; `--self` reports the first differing function body |

### P11 — Add `return`, `break`, `continue`, and `cond` [DONE — 2026-10-01]

```
Repo: AIPL. Add four control-flow forms in parser.rs, checker.rs, vm.rs, wasm.rs, and codegen.aipl: `(return v)` / `(return)` for void; `(break)` and `(continue)` valid only inside while/loop (checker error otherwise); `(cond (c1 e1) (c2 e2) ... (else e))` as sugar desugared in the parser to nested if. wasm lowering: wrap each function body in a block with the function's result type so `return` is `br` to it (or use the `return` instruction); loops become block{loop{...}} where break = br 1 and continue = br 0 (for `loop`, continue must still execute the increment — restructure the increment to sit at the top of the loop guarded by a first-iteration flag or emit the increment in a nested block so continue targets it). VM: implement via a ControlFlow enum returned from eval_expr (Normal(Value) | Break | Continue | Return(Value)) instead of panicking or using errors. Then rewrite the `(let done:bool false) (while (not done) ...)` loops in aipl_src/compiler.aipl and aipl_src/codegen.aipl to use break, and the 3+-deep nested ifs to cond. Update AIPL_SPEC.md and PROMPT_GUIDE_FOR_AIS.md. All existing tests must pass, plus differential cases for early return inside loop, break inside nested if, continue in `loop`.
```

**Verification 2026-10-01 (branch `features/p11`).**

| Requirement | Status | Evidence |
|---|---|---|
| `(return v)`/`(return)`, `(break)`, `(continue)` (loop bodies only), `cond` desugared to nested `if` | Done | `cond` requires `else` (as `if` does) and takes multi-expression clauses; the three statements are typed `void` |
| wasm: return, block/loop with break = br to block, continue = br to header; `loop` continue still runs the increment | Done | `return` instruction; the `loop` body is wrapped in a block that `continue` targets, falling into the step; label depths tracked |
| VM via a ControlFlow enum from eval_expr | Done differently | A pending-flow field checked by statement sequences, loops, and `invoke`; equivalent under the void typing, far smaller change |
| Rewrite done-flag loops to break, 3+-deep ifs to cond in compiler.aipl and codegen.aipl | Done | Also `std/*`; search loops became early returns |
| Update AIPL_SPEC.md and PROMPT_GUIDE_FOR_AIS.md | Done | Spec 7.10, grammar, tables, diagnostics, pitfalls; prompt guide rule 7 and the binary-search example |
| All tests pass + differential cases: early return in loop, break in nested if, continue in loop | Done | `tests/test_control_flow.rs`, plus self-hosted parity on raw `cond` |
| Found on the way | Fixed | VM `loop` evaluated end/step once and ignored `set!` of the variable (wasm re-evaluates); duplicate struct fields were accepted |

### P12 — Version everything and spec the binary AST or delete it [BINARY AST DELETED; VERSIONING DEFERRED — 2026-10-01]

```
Repo: AIPL. Add a language version: `(module name :version 1)` (parser accepts optional `:version N` after the name; missing = error "module must declare :version"). Store it in Module and reject any version the toolchain does not support. Emit a wasm custom section "aipl.version" containing the version and the toolchain git SHA. For src/compiler/binary_ast.rs: either (a) replace rmp_serde with a hand-written tag-based encoder/decoder with a 4-byte magic "BAPL", a u16 format version, and explicit numeric tags per Expr/OpCode variant defined in a table in ast.rs (so enum reordering cannot break files), with a roundtrip test over every example file; or (b) delete binary_ast.rs, the binary-encode/decode CLI subcommands, and the `.baipl` mentions in README/AIPL_SPEC. Choose (b) unless PROMPT_GUIDE_FOR_AIS.md gives a concrete token-savings measurement justifying (a); if you choose (a), include that measurement in the PR description.
```

**Status 2026-10-01 (branch `features/p12`).**

- **Binary AST: option (b), deleted.** `src/compiler/binary_ast.rs`, the `binary-encode`/`binary-decode` subcommands, the `rmp-serde` dependency, its round-trip test, and the benchmark's payload line are gone, and the spec's "dual representation" section is rewritten. The format was the serde layout of `src/ast.rs` (any enum reorder broke every file), no measurement of token savings existed (the prompt's condition for keeping it), and nothing depended on it. Text is the interchange format; `src/printer.rs` gives a canonical flat form of any resolved program.
- **Language versioning: deferred by decision.** A mandatory `:version` would touch every `.aipl` file and hundreds of inline test programs for a language with one implementation, one repository, and no external users; the git SHA in a wasm custom section would also break byte parity with the self-hosted compiler (which cannot know the SHA) and make builds non-reproducible. Revisit when there are packages from different authors or a second toolchain. A cheap step then: an optional `:version N` (missing = current), unknown versions rejected, no SHA.

### P13 — Retire the toy ELF backend; state the native strategy [DONE — 2026-10-01]

```
Repo: AIPL. The x86_64 ELF emitter cannot compile control flow, function calls, or memory access (attic/elf_emitter.aipl has no labels, relocations, stack frames, or jumps, hardcodes p_filesz=256, and encodes 32-bit cmpxchg as 0f b0). Do not extend it. Instead: (1) add `aipl build-native <file.aipl> -o <exe>` to src/main.rs that compiles to wasm via the existing backend, then shells out to `wasmtime compile` (or wasm2c + cc if wasmtime is absent) and errors clearly if neither tool is installed; (2) write docs/NATIVE_TARGET.md stating: native = wasm AOT for now; a true native backend will be built as a wasm->x86_64 lowering in AIPL only after codegen.aipl self-compiles byte-identically (P9's --self check), because wasm is the single semantic reference (P3); (3) remove every "bare-metal", "dual-target", and "ELF64" claim from README.md, AIPL_SPEC.md, PROMPT_GUIDE_FOR_AIS.md, and the clap `about` string in src/main.rs. Do not write any x86 encoding code.
```

**Verification 2026-10-01 (branch `features/p13`).**

| Requirement | Status | Evidence |
|---|---|---|
| (1) `aipl build-native` shelling out to `wasmtime compile` or wasm2c + cc | Not done, by decision | `wasmtime compile` yields a `.cwasm` that still runs inside wasmtime, not an executable; wasm2c needs a C toolchain plus a WASI runtime library; neither tool is installed here, so the command could not be tested. Both paths need nothing from AIPL beyond the `.wasm`, so `docs/NATIVE_TARGET.md` documents the commands instead |
| (2) `docs/NATIVE_TARGET.md`: native = wasm AOT; any future native backend is a wasm-to-native lowering in AIPL after self-hosting | Done | Includes measured numbers (fib(27) ~1 ms under wasmtime vs ~460 ms in the VM) |
| (3) Remove "bare-metal", "dual-target", "ELF64" claims from README, spec, prompt guide, CLI `about` | Done | Also the spec subtitle, the web page title, two module headers, and compiler.aipl's dead `compile_to_target`/`compile_aipl` stubs (they returned -1 and advertised an ELF target). Remaining mentions are in `attic/` and this audit's history |
| No x86 encoding code written | Done | |

### P14 — Resolver in AIPL (unblocked by P6, P8, P9) [DONE — 2026-10-01, `src/resolver.rs` kept]

**Completion 2026-10-01 (branch `features/p14`).** `aipl_src/resolver.aipl` matches `src/resolver.rs` (tested by compiled-byte equality over every multi-module program in the repository, aliases, diamonds, struct names, cycles, missing modules), `aipl compile --self` uses it, and `aipl_src/driver.aipl` (resolver + codegen) compiled to wasm reproduces itself under wasmtime and runs as a WASI command (args and environment ops added for it: `args.*`, `env.*`, `std/os`). Deviations from the prompt below, each deliberate: the output is flat source text, not a merged AST, because that is what `codegen.compile_module` consumes; the search path is the spec's (importer, entry, then library directories passed by the host), not `aipl_modules/`; and `src/resolver.rs` is **not** deleted, because `verify`/`eval`/`compile`/`test` hand the Rust checker a Rust `Module`, which an AIPL resolver cannot produce until the checker is in AIPL too. It stays as the oracle the tests compare against. Needed along the way: string literals moved out of the 512-byte area (literals at 1024, heap after them), the memory cap raised to 1024 pages, and a non-short-circuit `and` bug in codegen.aipl fixed (PROGRESS.md has the details).

Original prompt:

```
Repo: AIPL. Rewrite src/resolver.rs as aipl_src/resolver.aipl using fs.open/fs.read (WASI-backed after P6) and the struct types from P8. Behavior must match resolver.rs exactly: locate `<name>.aipl` in the importer's directory then aipl_modules/; parse with compiler.parse_ast; detect cycles (in_progress set) and diamonds (included set); rename every fn in an imported module to `<import>.<fn>`, rewrite its internal `(call f)` and `(ref f)` targets and its alias-qualified calls to canonical names; the entry module's functions keep bare names. Output is a merged AST in memory that codegen.emit_module consumes. Add tests mirroring tests/test_v2.rs::test_v2_multi_module_linkage and the circular-import error. Once `aipl compile --self` passes on test_suite.aipl using resolver.aipl, delete src/resolver.rs and route main.rs through the VM-hosted resolver.aipl. Update the TEMPORARY header's promise in PROGRESS.md as fulfilled.
```
