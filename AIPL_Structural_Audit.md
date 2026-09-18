# AIPL Structural Audit

Audit date: 2026-09-17. Tree at commit `c97a10c`. Every claim below is anchored to a file and line in this repo; nothing under `target/` was consulted.

Companion documents: [LANGUAGE_GAPS.md](LANGUAGE_GAPS.md) (what the language does not yet do), [PROGRESS.md](PROGRESS.md) (what has been built and verified). This document is the execution roadmap: what is structurally sound, what is debt, what will break at scale, and the ordered list of tasks (with ready-to-run agent prompts) to fix it.

---

## 1. THE GOOD

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

## 4. MASTER PRIORITIES & AI PROMPTS

Ordered by dependency and leverage. Each prompt is self-contained and ends in a runnable check, so "done" means the check passed — not that a number came back.

**Sequence:** P1 → then P2, P3, P4 in parallel → P5 before P9 → P6 before P14 → P7 and P8 before P9's `--self` check → P10–P13 after P9.

---

### P1 — Quarantine fabrications and delete the tests that certify them [DONE]

```
Repo: AIPL. Move aipl_src/sovereign_toolchain.aipl, aipl_src/pipeline.aipl, aipl_src/elf_emitter.aipl, aipl_src/optimizer.aipl, aipl_src/diagnostics.aipl, aipl_src/aipl_test.aipl, aipl_src/aipl_db.aipl into a new attic/ directory with a one-paragraph attic/README.md stating these modules return constants instead of doing work (cite: sovereign_toolchain.aipl fs_open returns 10/11, fs_read returns count, thread_spawn_sync returns 101, tokenize counts parens only; pipeline.aipl pipeline_tokenize returns 4 on zero tokens; compiler.aipl emit_wasm_binary reads opcode from the next_sibling field at ast_ptr+12). Delete src/bin/aipl_test_runner.rs and src/bin/aisql_runner.rs and their [[bin]] entries in Cargo.toml. In tests/test_v2.rs delete every #[test] that loads any attic module (test_self_hosted_wasm_emitter, test_sovereign_aipl_diagnostics, test_sovereign_aipl_test_runner, test_dual_target_native_elf_emitter, test_heavy_optimizer_constant_folding, test_sovereign_wasm_roundtrip_execution, test_e2e_sovereign_pipeline_bootstrap). In aipl_src/compiler.aipl delete emit_wasm_binary, compile_to_target's ELF branch, and aipl_heap_alloc (it duplicates memory.aipl); make compile_to_target return -1 with a comment "not yet wired to codegen.aipl". Run `cargo test` and `aipl test aipl_src/test_suite.aipl`; both must pass. Update LANGUAGE_GAPS.md and PROGRESS.md to list what moved and why. Do not rewrite or "fix" any attic module. Do not add new functionality.
```

### P2 — Eliminate every silent catch-all; make every OpCode either implemented or rejected [DONE]

```
Repo: AIPL. Remove the four silent fallbacks: src/vm.rs eval_op `_ => Ok(Value::Int(0))`, src/compiler/wasm.rs `_ => Nop` (two sites, in the Op match and the Expr match), src/checker.rs `_ => Ok(Type::I32)`, src/compiler/wasm.rs aipl_to_wasm_type `_ => ValType::I32`. Replace each with an explicit match arm per variant; unsupported variants return Err("<op> not supported in <VM|wasm backend>: <reason>") — never a default value or nop. Implement OpCode::Mod for real in VM (i32 rem, error on zero) and wasm (I32RemS). Implement MemAlloc in wasm as a bump allocator using a wasm global initialized to 1024 (add a GlobalSection; emit global.get/i32.add/global.set). Implement SysPrint in wasm as a hard error for now (comes with WASI in P6). Add tests/test_opcode_conformance.rs with one test that, for every OpCode variant in src/ast.rs, parses a minimal program using it and asserts it is exactly one of: (a) VM ok AND wasm compiles AND wasm validates via wasmparser, or (b) rejected with an Err at check or compile time. Any variant that silently succeeds with a default value fails the test. Remove OpCode variants that no implementation will support this quarter (VecDot, MatMul, DomElem, DomMount, DomAppend, DomOnEvent, WebAlert) from ast.rs, parser.rs, checker.rs and AIPL_SPEC.md. Add wasmparser as a dev-dependency for validation. Run cargo test.
```

### P3 — Define integer semantics once; add VM-vs-wasm differential testing

```
Repo: AIPL. Make `i32` mean wrapping 32-bit in both backends. In src/vm.rs, every arithmetic/bitwise op on Value::Int for operands typed i32 must apply `as i32` wrapping (wrapping_add/sub/mul, i32 div with zero and MIN/-1 errors, shl/shr masked to 5 bits, shr = arithmetic). Add OpCode::ShrU (parser "shru", wasm I32ShrU) and OpCode::DivU/RemU ("divu","remu"). Add OpCode::I64 variants only if the checker actually distinguishes them; otherwise document that i64 is unsupported and reject `i64` in parse_type. Then add tests/test_differential.rs: for each file in examples/*.aipl and aipl_src/codegen.aipl's test functions, run the named zero-arg i32 function in the VM and in wasmtime (add `wasmtime` as a dev-dependency) and assert equal results. Include explicit cases for: (+ 2147483647 1), (shr -8 1), (* 65536 65536), (/ -7 2), (% -7 2), loop with end bound inclusive, while with set! in body. Fix every divergence in the VM, not the wasm backend — wasm is the reference from now on. Record the decision "wasm semantics are the spec" at the top of AIPL_SPEC.md.
```

### P4 — Source positions on every diagnostic

```
Repo: AIPL. Make every parser and checker error carry file:line:col. In src/parser.rs change Token to a struct { kind: TokenKind, line: u32, col: u32 } (tokenize tracks line/col by counting '\n'); every Err in Parser::* must include the position of the offending token as "<line>:<col>: <message>". Add span (line, col) to Expr::Let/Set/If/Loop/While/Call/Op via a wrapping `Spanned<Expr>` or a `span` field on FnDef + each Expr variant — pick the smaller change and apply it consistently. In src/checker.rs every Err must include the span of the expression being checked. In src/resolver.rs prefix errors with the file path. Detect and error on: unterminated string literal; tokens remaining after the module's closing paren ("unexpected tokens after module end at L:C — check for an extra ')'"); a symbol named inf/nan/infinity being parsed as a float (make float literals require a digit and a '.'). Add tests/test_diagnostics.rs asserting exact "L:C:" prefixes for: missing ')', unknown op, type mismatch in let, undefined variable, extra ')' after module. Update PROGRESS.md.
```

### P5 — One memory layout, one allocator, no hardcoded table addresses

```
Repo: AIPL. Unify memory. Define in AIPL_SPEC.md a fixed layout: bytes 0-1023 reserved for the runtime (0: heap_ptr word, 4: compile_error flag, 8: reserved...), heap begins at 1024. In src/vm.rs remove SharedMemory.heap_ptr and make OpCode::MemAlloc read/write the i32 at address 0 (initialize to 1024 in VM::new). In wasm.rs MemAlloc must read/write address 0 likewise (i32.load 0 / i32.store 0) so VM and compiled code share the same cursor. Rewrite aipl_src/memory.aipl aipl_heap_alloc as a thin wrapper `(mem.alloc size)` and delete the duplicate in compiler.aipl. In aipl_src/codegen.aipl replace every hardcoded table base (20000 keywords, 30000 function table, 40500 locals count, 40600 error flag, 41000 locals table) with pointers obtained from (mem.alloc N) at init time and stored in a documented runtime block at fixed cells 16..64 (e.g. mem[16]=keywords_ptr, mem[20]=fn_table_ptr, ...). Grep the file for every literal in 20000-41999 and confirm zero remain. Add `memory.grow` as OpCode::MemGrow (VM: extend Vec by pages*65536; wasm: MemoryGrow). Run codegen.aipl's tests via `aipl test aipl_src/codegen.aipl --func test_compile_compute` and the full suite; both must pass.
```

### P6 — WASI imports and data segments: compiled AIPL that can do I/O

```
Repo: AIPL, src/compiler/wasm.rs. Add an ImportSection importing from "wasi_snapshot_preview1": fd_write, fd_read, path_open, fd_close, proc_exit, with correct WASI signatures. Function indices for user functions must shift by the import count. Lower: sys.print(str) -> write bytes via fd_write to fd 1 using an iovec scratch area at fixed address 64; fs.open(ptr,len,flags) -> path_open on preopened dir fd 3 with oflags derived from flags (0=read, else create|trunc) and rights for read/write, returning the new fd or -1; fs.read/fs.write -> fd_read/fd_write with a single iovec; fs.close -> fd_close; sys.exit -> proc_exit. Emit a DataSection: every Literal::Str is interned once, placed at addresses starting at 512 (below heap start 1024; error if total exceeds 512 bytes for now), and the literal compiles to i32.const <addr> with its length available via a new op (str.len s) -> i32 that the checker types as i32. Change Type::Str lowering to i32 (pointer) and document that. Remove the "not yet supported" errors for Fs* ops. Add tests/test_wasi.rs that compiles aipl_src/file_io.aipl's run_file_io_tests, runs it under wasmtime with a preopened temp dir and the WASI ctx, and asserts the same result as the VM. Do not touch thread.* in this task.
```

### P7 — Fix statement/expression typing and scoping in the spec and all three implementations

```
Repo: AIPL. Decide and implement these rules identically in src/checker.rs, src/vm.rs, src/compiler/wasm.rs, and aipl_src/codegen.aipl: (1) `set!` has type void. (2) `if` whose two branches are both void is void; otherwise both branches must have the same non-void type — an if mixing void and non-void is a type error with a message suggesting `(block ... value)`. (3) `let` has type void (it declares, it does not yield); a function body's last expression must therefore be a value expression when the return type is non-void. (4) `let` is block-scoped: a `let` inside if/while/loop/block/match arms is visible only within that construct; shadowing an outer name is a type error. (5) `set!` on an undeclared name is a type error and a VM runtime error (delete the globals fallback at vm.rs Expr::Set). (6) match_result arms bind ok_var/err_var to the actual Ok/Err payload types from the matched expression's ResultType; `ok`/`err` take an explicit result type via (ok:T v) or infer from an enclosing let/return type — pick one and implement it. Remove the `(block (set! done true) 0)` idiom from codegen.aipl and compiler.aipl now that void ifs are legal. Update AIPL_SPEC.md with these six rules verbatim. Extend tests/test_differential.rs with a case per rule.
```

### P8 — Structs and real arrays: kill manual offset arithmetic

```
Repo: AIPL. Add aggregate types. Grammar: module-level `(struct Name [f1:type f2:type ...])`; expressions `(new Name)` -> i32 pointer via mem.alloc of the struct size; `(get p Name.f)` and `(put p Name.f v)` which lower to i32.load/i32.store (or load8/store8/load64 by field type) at compile-time offset; `(sizeof Name)`. Arrays: `(arr.new type n)` -> pointer; `(arr.get type p i)` / `(arr.set type p i v)` lowering to base + i*sizeof(type) with a VM bounds check against the allocation length stored in the 4 bytes before the base. Implement in parser.rs (new Module field structs: Vec<StructDef>), checker.rs (field lookup and type of get/put), vm.rs (same layout as wasm — pointers are ints, fields at the same offsets), wasm.rs. Then port aipl_src/compiler.aipl's token record ([kind,a,b] 12 bytes) and AST node ([kind,a,b,next] 16 bytes) to `(struct Token ...)`/`(struct Node ...)` and replace every `(mem.load32 (+ ptr (+ (* idx 16) (* field 4))))` with get/put. All existing compiler.aipl tests must still pass via `aipl test aipl_src/test_suite.aipl`. Document the struct layout rule (fields in declaration order, natural alignment, no padding beyond alignment) in AIPL_SPEC.md.
```

### P9 — Finish module assembly in codegen.aipl and wire it as the compiler entry

```
Repo: AIPL, aipl_src/codegen.aipl and aipl_src/compiler.aipl. Implement `emit_module [ast_root:i32 out_ptr:i32] -> i32` in codegen.aipl: walk the module's fn children (use collect_functions), emit type section (one type per function, dedup optional), function section, memory section (min 1 max 100, export "memory"), export section (every fn by name), and code section where each body is compiled via compile_function_body into a scratch buffer, then prefixed by its LEB128 length and a locals vector (group consecutive same-valtype locals as the wasm format requires). Every section is written into a scratch buffer first and then copied after its LEB128 size prefix — no hardcoded lengths anywhere. Then make compiler.aipl `(import codegen)` and have compile_to_target call codegen.emit_module for target_flag 0 and return -1 for any other target. Add test functions in codegen.aipl that compile (a) a two-function module where one calls the other and (b) a module with 3 locals of mixed i32/bool, write them via fs.open/fs.write to aipl_src/_out_*.wasm, and add a Rust test that validates both with wasmparser and executes them in wasmtime with expected results; the test deletes the files afterward. Finally add `aipl compile --self <file>`: compile <file> with the Rust backend AND with codegen.aipl running in the VM, validate both, and run every exported zero-arg function in both under wasmtime, asserting equal results. Run it on aipl_src/memory.aipl and aipl_src/file_io.aipl.
```

### P10 — First-class function references; fix thread.spawn under imports

```
Repo: AIPL. Add a function-reference type and indirect calls. Grammar: type `(fn [t1 t2] -> r)`; expression `(ref name)` yields an i32 table index; `(call_ref f args...)` invokes it. wasm.rs: emit a TableSection + ElementSection listing every function, and lower call_ref to call_indirect with the function's type index. vm.rs: represent refs as Value::Int(index) into a Vec<FnDef> built at load_module; call_ref looks up by index. resolver.rs: rewrite `(ref name)` targets exactly like Call targets (add Expr::Ref to walk_calls_expr). Change thread.spawn's signature to (thread.spawn fref:i32 arg:i32) -> i32 taking a function index, delete the read-name-from-memory path in vm.rs ThreadSpawn, update aipl_src/thread_sync.aipl accordingly, and add `(import thread_sync)` plus a report line to aipl_src/test_suite.aipl. Delete the explanatory comment block in test_suite.aipl about thread_sync exclusion. Run `aipl test aipl_src/test_suite.aipl`; the thread group must pass with 4000 as before.
```

### P11 — Add `return`, `break`, `continue`, and `cond`

```
Repo: AIPL. Add four control-flow forms in parser.rs, checker.rs, vm.rs, wasm.rs, and codegen.aipl: `(return v)` / `(return)` for void; `(break)` and `(continue)` valid only inside while/loop (checker error otherwise); `(cond (c1 e1) (c2 e2) ... (else e))` as sugar desugared in the parser to nested if. wasm lowering: wrap each function body in a block with the function's result type so `return` is `br` to it (or use the `return` instruction); loops become block{loop{...}} where break = br 1 and continue = br 0 (for `loop`, continue must still execute the increment — restructure the increment to sit at the top of the loop guarded by a first-iteration flag or emit the increment in a nested block so continue targets it). VM: implement via a ControlFlow enum returned from eval_expr (Normal(Value) | Break | Continue | Return(Value)) instead of panicking or using errors. Then rewrite the `(let done:bool false) (while (not done) ...)` loops in aipl_src/compiler.aipl and aipl_src/codegen.aipl to use break, and the 3+-deep nested ifs to cond. Update AIPL_SPEC.md and PROMPT_GUIDE_FOR_AIS.md. All existing tests must pass, plus differential cases for early return inside loop, break inside nested if, continue in `loop`.
```

### P12 — Version everything and spec the binary AST or delete it

```
Repo: AIPL. Add a language version: `(module name :version 1)` (parser accepts optional `:version N` after the name; missing = error "module must declare :version"). Store it in Module and reject any version the toolchain does not support. Emit a wasm custom section "aipl.version" containing the version and the toolchain git SHA. For src/compiler/binary_ast.rs: either (a) replace rmp_serde with a hand-written tag-based encoder/decoder with a 4-byte magic "BAPL", a u16 format version, and explicit numeric tags per Expr/OpCode variant defined in a table in ast.rs (so enum reordering cannot break files), with a roundtrip test over every example file; or (b) delete binary_ast.rs, the binary-encode/decode CLI subcommands, and the `.baipl` mentions in README/AIPL_SPEC. Choose (b) unless PROMPT_GUIDE_FOR_AIS.md gives a concrete token-savings measurement justifying (a); if you choose (a), include that measurement in the PR description.
```

### P13 — Retire the toy ELF backend; state the native strategy

```
Repo: AIPL. The x86_64 ELF emitter cannot compile control flow, function calls, or memory access (attic/elf_emitter.aipl has no labels, relocations, stack frames, or jumps, hardcodes p_filesz=256, and encodes 32-bit cmpxchg as 0f b0). Do not extend it. Instead: (1) add `aipl build-native <file.aipl> -o <exe>` to src/main.rs that compiles to wasm via the existing backend, then shells out to `wasmtime compile` (or wasm2c + cc if wasmtime is absent) and errors clearly if neither tool is installed; (2) write docs/NATIVE_TARGET.md stating: native = wasm AOT for now; a true native backend will be built as a wasm->x86_64 lowering in AIPL only after codegen.aipl self-compiles byte-identically (P9's --self check), because wasm is the single semantic reference (P3); (3) remove every "bare-metal", "dual-target", and "ELF64" claim from README.md, AIPL_SPEC.md, PROMPT_GUIDE_FOR_AIS.md, and the clap `about` string in src/main.rs. Do not write any x86 encoding code.
```

### P14 — Resolver in AIPL (unblocked by P6, P8, P9)

```
Repo: AIPL. Rewrite src/resolver.rs as aipl_src/resolver.aipl using fs.open/fs.read (WASI-backed after P6) and the struct types from P8. Behavior must match resolver.rs exactly: locate `<name>.aipl` in the importer's directory then aipl_modules/; parse with compiler.parse_ast; detect cycles (in_progress set) and diamonds (included set); rename every fn in an imported module to `<import>.<fn>`, rewrite its internal `(call f)` and `(ref f)` targets and its alias-qualified calls to canonical names; the entry module's functions keep bare names. Output is a merged AST in memory that codegen.emit_module consumes. Add tests mirroring tests/test_v2.rs::test_v2_multi_module_linkage and the circular-import error. Once `aipl compile --self` passes on test_suite.aipl using resolver.aipl, delete src/resolver.rs and route main.rs through the VM-hosted resolver.aipl. Update the TEMPORARY header's promise in PROGRESS.md as fulfilled.
```
