# Comprehensive Architectural & Compiler Engineering Audit: AIPL

**Auditor:** Principal Systems Architect & Compiler Engineer  
**Target Repository:** `/home/matt/projects/AIPL`  
**Target Git Branch / Commit:** `gemini-audit` (`7c67971`)  
**Rust Toolchain:** `rustc 1.99.0 (b940084d7 2026-09-28)` / Linux x86-64  
**Date:** October 5, 2026  

---

## 1. Executive Technical Assessment

### Architectural Health & System Topology
The AIPL (AI Programming Language) project represents an ambitious, multi-tiered compiler architecture comprising three distinct execution and translation pipelines:
1. **Bootstrap Reference Pipeline (Rust):** An S-expression reader (`src/sexpr.rs`), generic expander (`src/generics.rs`), const/enum desugarer (`src/consts.rs`), recursive-descent parser (`src/parser.rs`), static type checker (`src/checker.rs`), tree-walking AST interpreter (`src/vm.rs`), and a direct WebAssembly bytecode emitter (`src/compiler/wasm.rs`).
2. **Self-Hosted Bootstrap Pipeline (AIPL-native):** A parallel toolchain written in AIPL itself (`aipl_src/`: `driver.aipl`, `resolver.aipl`, `generics.aipl`, `consts.aipl`, and `codegen.aipl`) that emits WebAssembly bytecode directly using manual LEB128 and binary section serialization.
3. **Native x86-64 Backend (AIPL-native):** A custom translator written in AIPL (`aipl_src/native/`: `wasm_reader.aipl`, `lower.aipl`, `x64.aipl`, `elf.aipl`, `runtime.aipl`, `wasi.aipl`) that reads compiled WASM binaries and lowers them into standalone ELF64 Linux executables without an external C runtime or assembler.

### Current Maturity Level: *Hardened Pre-Alpha Compiler with Severe Semantic Asymmetries*
The codebase demonstrates genuine technical achievements: the self-hosted compiler achieves byte-for-byte fixpoint reproducibility (`stage1.wasm` == `stage2.wasm`), passes differential execution across 38 cross-backend suites, and emits functioning ELF64 binaries that link Linux syscalls directly. 

However, AIPL cannot be classified as a production-grade or memory-safe systems compiler. It is a **hardened pre-alpha dialect with serious semantic divergence between execution modes**. Beneath the surface of passing test suites lie critical compiler crashes, type checker holes, memory leaks by design, and a fundamentally illusory safety model where contracts and array bounds checks are silently stripped when targeting WASM and native machine code.

### Key Technical Risks
1. **Compiler Panics & Verification Bypass:** The type checker (`src/checker.rs`) contains incomplete match arms that completely omit arity and type checks for atomic intrinsics and thread operations, allowing malformed programs to pass `aipl verify` and crash the compiler or VM with raw index out-of-bounds panics.
2. **The "Safety Illusion" (Execution Divergence):** The VM enforces pre/post-conditions (`req`/`ens`) and dynamic array bounds checks. The WASM compiler and Native backend completely discard contracts and lower array accesses to raw unchecked pointer offsets. Code verified in `aipl eval` as "safe" will silently execute out-of-bounds reads/writes or memory corruption when compiled to WASM or native ELF binaries.
3. **Absence of Memory Reclamation:** The runtime possesses no garbage collector, reference counting, or free-list allocator. Linear memory operates on a strictly monotonic bump cursor capped at 64 MiB (1,024 WASM pages). `mem.free` is a dead no-op in Rust and a compile-time crash in the self-hosted compiler. The standard library leaks memory by design on basic operations (strings, formatters, maps).
4. **Massive Stack Consumption:** Recursive parsing and AST tree-walking necessitate allocating an extraordinary **1 GiB thread stack** (`src/main.rs:233`) and a **256 MiB stack** (`src/resolver.rs:77`) to prevent segmentation faults during normal execution.
5. **Hollow Conformance Assertions:** Certain regression tests (notably `tests/test_opcode_conformance.rs:181-189`) pass vacuously whenever an opcode fails on either backend, masking implementation deficiencies.

---

## 2. Feature Status Matrix

| Feature / Construct | Spec Status (`AIPL_SPEC.md`) | Implementation Status (VM / WASM / Self-Host / Native) | Test Coverage | Operational Reality / Verdict |
| :--- | :--- | :--- | :--- | :--- |
| **Integer Arithmetic (`i32`, `i64`)** | Fully specified (§2, §4.A) | Complete across all 4 targets | High (`test_differential.rs`, `test_primitives.rs`) | **Production Ready.** Correct two's complement 32/64-bit wrapping arithmetic and bitwise ops. |
| **Float Arithmetic (`f32`, `f64`)** | Fully specified (§2, §4.A) | Complete in VM, Rust WASM, Self-Host, Native | Moderate (`test_floats.rs`, `test_f64_literals.rs`) | **Working.** Float literals restricted to $\le 2^{53}$ in self-host (§6.4). Non-standard operations (`%`, bitwise) properly rejected. |
| **Floating-Point Memory (`mem.load_f32/f64`, `mem.store_f32/f64`)** | Defined in grammar (§2) | **Broken.** VM returns `Err`; Rust WASM returns `Err`; Self-host crashes with error 987 | Untested / Negative-only | **Spec-Only Fiction.** Present in grammar, but completely rejected across all backends. Users cannot load/store raw floats in memory buffers. |
| **String Type (`str`)** | Specified (§2, §4.B) | VM: dynamic string; WASM/Native: interned data segment + ptr/len | High (`test_strings.rs`) | **Partial / Divergent.** Literals, `str.len`, `str.ptr` work. String concatenation (`+ str str`) is **VM-only**; fails at compile-time in WASM/Native. |
| **Structs (`struct`, `ptr S`, `new`, `get`, `put`)** | Specified (§2, §4.C) | Complete across all targets | High (`test_structs.rs`, `test_generics.rs`) | **Working.** Monomorphic byte-offset layout. Pointers are non-nullable raw memory offsets. |
| **Arrays (`arr T`, `arr.new`, `arr.get`, `arr.set`, `arr.len`)** | Specified (§2, §4.E) | Complete, but **divergent safety semantics** | High (`test_arrays.rs`, `test_differential.rs`) | **Soundness Hazard.** VM bounds-checks accesses. WASM and Native compile to unchecked pointer offsets; out-of-bounds access silently corrupts memory. |
| **Enums & Consts (`enum`, `const`)** | Specified (§4.I) | Pre-pass token rewriters (`src/consts.rs`, `aipl_src/consts.aipl`) | High (`test_enums.rs`, `test_consts.rs`) | **Working.** Pure syntactic macro-expansion prior to type checking. No runtime overhead. |
| **Result Type (`Result<T,E>`, `ok`, `err`, `match_result`)** | Specified (§2, §4.F) | Complete across all targets | High (`test_results.rs`) | **Working.** Lowered to 8-byte heap-allocated tagged cells `[tag: i32, payload: i32]`. 64-bit payloads unsupported. |
| **Function References (`ref`, `call_ref`, `(fn [...] -> r)`)** | Specified (§2, §4.G) | Complete in VM, WASM (funcref table), Native | High (`test_funcref.rs`, `test_differential.rs`) | **Working.** Indirect calls via index table in WASM and address jump tables in Native. |
| **Generics (Parametric Polymorphism)** | Specified (§4.H) | Pre-pass S-expression template instantiator | High (`test_generics.rs`, `test_selfhost.rs`) | **Fragile AST Rewriter.** Not a genuine polymorphic type system. Uninstantiated templates are never type-checked; type errors in dead templates are ignored. |
| **Contracts (`req`, `ens`, `inv`)** | Specified (§2, §7.6) | **VM-Only.** Completely stripped in WASM, Self-Host, and Native | Low / VM-only (`test_contracts.rs`) | **Illusory Safety.** `inv` is a total dead no-op. `req`/`ens` run dynamically in `aipl eval` only. Zero contract enforcement in compiled release binaries. |
| **Linear Memory (`mem.alloc`, `mem.grow`)** | Specified (§4.A) | Complete across all targets | High (`test_memory.rs`, `test_wasi.rs`) | **Working.** Atomic bump-pointer allocation. Page ceiling hard-limited to 1,024 pages (64 MiB). |
| **Memory Deallocation (`mem.free`)** | Defined in grammar (§2) | VM: no-op; WASM: no-op; Self-Host: **compiler crash (error 987)** | Hollow test in `test_opcode_conformance.rs` | **Broken & Contradictory.** Spec claims no-op; self-hosted compiler rejects with `unknown_form` error 987, breaking compiler parity. |
| **Concurrency (`thread.spawn`, `thread.join`)** | Specified (§4.D) | VM: OS threads; WASM: `wasi.thread-spawn`; Native: `clone` syscall | Moderate (`test_threads.rs`, `test_concurrency.rs`) | **Dangerous.** Type checker fails to validate handle type in `thread.join`. Host must support WASI threading extension. |
| **Hardware Atomics (`atomic.add/cas/lock/unlock`)** | Specified (§4.D) | VM: Mutex-guarded; WASM: WASM atomics; Native: x86 `lock` prefix | High (`test_atomics.rs`) | **Compiler Panic Bug.** Type checker has zero arity/type checks. `(atomic.add)` panics Rust compiler and VM with index out-of-bounds. |
| **WASI System I/O (`fs.*`, `args.*`, `env.*`)** | Specified (§10.5) | Complete in VM and WASI/Native | High (`test_wasi.rs`, `test_file_io.rs`) | **Working.** Implements sandboxed capability-based file descriptors via `openat2(RESOLVE_BENEATH)`. |
| **Terminal I/O (`sys.print`, `sys.exit`)** | Specified (§2, §4.B) | Complete, but **divergent type acceptance** | High (`test_wasi.rs`) | **Type Checker Drift.** Checker accepts any type; WASM codegen rejects anything other than `str`. `(sys.print 1)` passes `verify` but fails `compile`. |
| **System Clocks (`sys.time`, `sys.monotonic`, `sys.random`)** | Specified (§2) | Complete across all targets | Moderate (`test_wasi.rs`) | **Working.** Nanosecond system/monotonic time and OS entropy retrieval. |
| **Control Flow (`loop`, `while`, `cond`, `return`, `break`)** | Specified (§2) | Complete across all targets | High (`test_control_flow.rs`) | **Working.** Correct label resolution and break/continue stack tracking. `cond` desugars cleanly to nested `if`. |
| **Self-Hosted Toolchain (`aipl compile --self`)** | Specified (§1.2, §6.4) | Complete (`aipl_src/driver.aipl`) | High (`test_selfhost.rs`) | **Operational Milestone.** Byte-identical fixpoint self-compilation under Wasmtime. |
| **Native x86-64 Backend (`aipl native`)** | Specified (docs) | Complete (`aipl_src/native/native.aipl`) | High (`test_native.rs`) | **Working.** Emits self-contained ELF64 binaries without libc; supports WASI I/O and atomics. |

---

## 3. Documentation Drift & Inconsistencies

### 1. Type Checker vs. WASM Codegen Drift on `sys.print`
* **Contradiction:** `AIPL_SPEC.md:449` and `src/checker.rs:574-578` permit `sys.print` to take any argument types:
  ```rust
  OpCode::SysPrint => {
      for arg in args {
          self.infer_expr_type(arg, env)?;
      }
      Ok(Type::Void)
  }
  ```
  However, `src/compiler/wasm.rs:906-912` and `aipl_src/codegen.aipl` strictly enforce `str` arguments only:
  ```rust
  let t = expr_type(arg, ctx);
  if t != Type::Str {
      return Err("Wasm Codegen: sys.print supports str arguments only in the wasm backend".to_string());
  }
  ```
* **Impact:** The CLI promise that `aipl verify` guarantees compilation is broken. Running `aipl verify` on `(sys.print 123)` returns `OK: module type-checks`, but running `aipl compile` immediately fails with a codegen error.

### 2. Self-Hosted Compiler Rejection of `mem.free`
* **Contradiction:** `AIPL_SPEC.md:444` claims `mem.free` is a no-op across all backends. `src/compiler/wasm.rs:833-835` implements it as a no-op (`// No-op for bump allocator`). 
  However, in `aipl_src/codegen.aipl:154-209`, `mem.free` is completely missing from the keyword enum (`Kw`), causing the self-hosted compiler to abort with:
  ```text
  Error: "Self-host error: compile_module returned -1 with compile error 987 (see AIPL_SPEC.md 6.4)"
  ```
* **Impact:** Valid AIPL code containing `mem.free` compiles under the Rust backend but crashes under the self-hosted `--self` backend, breaking bootstrap equivalence.

### 3. Floating-Point Memory Operations
* **Contradiction:** `AIPL_SPEC.md:84` defines `mem.load_f32`, `mem.load_f64`, `mem.store_f32`, and `mem.store_f64` as standard grammar memory operations. `src/checker.rs:498-508` type-checks them successfully.
  Yet `src/compiler/wasm.rs:962-964` and `src/vm.rs:1623-1625` hard-reject them at runtime/compile-time with:
  ```rust
  Err("mem.load_f32 not supported in VM backend: floating point memory ops not implemented")
  ```
* **Impact:** The language specification advertises native float memory operations that do not exist anywhere in the executable backends.

### 4. Manifest Build Warning in `Cargo.toml`
* **Contradiction:** Cargo emits a manifest warning during every build invocation:
  ```text
  warning: Cargo.toml: file /home/matt/projects/AIPL/benches/execution_bench.rs found to be present in multiple build targets:
    * `bin` target `execution_bench`
    * `bench` target `execution_bench`
  ```
* **Location:** `Cargo.toml:27-28`. Declaring `benches/execution_bench.rs` as a `[[bin]]` while it resides in the conventional benchmark directory violates Cargo conventions.

---

## 4. Architectural & Soundness Vulnerabilities

### Vulnerability 1: Uncaught Compiler Panics via Missing Checker Arity Checks
* **Severity:** **CRITICAL / P0**  
* **Location:** `src/checker.rs:530-532`
* **Mechanism:** The type checker registers atomic opcodes with static return types but performs **zero validation** on argument count or argument types:
  ```rust
  OpCode::AtomicAdd => Ok(Type::I32),
  OpCode::AtomicCas => Ok(Type::Bool),
  OpCode::AtomicLock | OpCode::AtomicUnlock => Ok(Type::Void),
  ```
  When an empty expression `(atomic.add)` is supplied:
  1. `aipl verify` passes unconditionally.
  2. `src/compiler/wasm.rs:970` executes `compile_expr(&args[0], ctx, func)?;` without checking `args.len()`.
  3. `src/vm.rs:1069` executes `self.eval_expr(&args[0], scope)?` without checking `args.len()`.
* **Reproduction:**
  ```lisp
  (module crash (fn main [] -> i32 (atomic.add)))
  ```
  ```text
  $ aipl verify crash.aipl
  [AIPL Verifier] OK: module 'crash' type-checks
  $ aipl compile crash.aipl
  thread 'main' panicked at src/compiler/wasm.rs:970:27:
  index out of bounds: the len is 0 but the index is 0
  ```

### Vulnerability 2: Type Confusion in `thread.join`
* **Severity:** **HIGH / P0**  
* **Location:** `src/checker.rs:691-697`
* **Mechanism:** The checker verifies that an argument exists and evaluates its type, but **never asserts that the type is `Type::I32`**:
  ```rust
  OpCode::ThreadJoin => {
      if args.len() != 1 {
          return Err(format!("{}:{}: thread.join requires 1 argument (thread handle)", l, c));
      }
      self.infer_expr_type(&args[0], env)?; // <-- Ignores return type!
      Ok(Type::I32)
  }
  ```
* **Reproduction:** Passing a string `(thread.join "corrupt")` passes `aipl verify`. In the VM, it fails at runtime with `thread.join requires Int handle`. In compiled WASM, it passes an arbitrary memory pointer address as a thread handle directly to the host runtime.

### Vulnerability 3: The Boundary Safety Illusion (Stripped Contracts & Bounds Checks)
* **Severity:** **HIGH / P1**  
* **Location:** `src/compiler/wasm.rs`, `aipl_src/codegen.aipl`, `AIPL_SPEC.md:986`
* **Mechanism:** The language claims support for formal Design-by-Contract (`req`, `ens`, `inv`) and safe arrays. 
  In practice:
  - `inv` is a dead keyword (never evaluated anywhere).
  - `req` and `ens` are purely evaluated by the VM tree-walker. The WASM backend and Native code generator **completely discard all contract AST nodes**.
  - `arr.get` and `arr.set` perform bounds checks in the VM, but compile to raw, unchecked pointer arithmetic in WASM (`Instruction::I32Load` / `Instruction::I32Store`) and native x86-64.
* **Impact:** Differential execution hides failures because tests allow WASM success when VM traps on contracts or array bounds (`tests/test_differential.rs:18-20`). Developers testing code with `aipl eval` receive an erroneous guarantee of safety that is completely absent in production binaries.

### Vulnerability 4: Rampant Stack Allocation for AST Operations
* **Severity:** **MEDIUM / P1**  
* **Location:** `src/main.rs:233`, `src/resolver.rs:77`
* **Mechanism:** The Rust driver allocates a **1 GiB virtual stack** for the main thread and a **256 MiB stack** for module resolution:
  ```rust
  let builder = std::thread::Builder::new()
      .name("aipl-main".into())
      .stack_size(1 << 30); // 1 GB stack
  ```
  This is required because the AST parser, module resolver, and interpreter VM are written using un-trampolined recursive descent. Deeply nested S-expressions or extensive module import graphs blow the default Linux thread stack (8 MiB) instantly.

### Vulnerability 5: Hollow Test Assertion in Opcode Conformance Suite
* **Severity:** **MEDIUM / P2**  
* **Location:** `tests/test_opcode_conformance.rs:181-189`
* **Mechanism:** The opcode differential conformance test contains the following assertion logic:
  ```rust
  let option_a = if let (Ok(vm_val), Ok(wasm_bytes)) = (&vm_res, &wasm_res) { ... };
  let option_b = vm_res.is_err() || wasm_res.is_err();
  assert!(option_a || option_b);
  ```
  If an opcode fails in the VM *or* fails in the WASM backend, `option_b` evaluates to `true`, and the test passes. As a result, opcodes that are broken or unimplemented on one backend are reported as passing "conformance".

---

## 5. Immediate Action Plan

### Priority 0: Critical Fixes & Compiler Soundness (Immediate)
- [ ] **Patch `src/checker.rs` Atomic Opcodes:** Implement arity and type checking for all atomics:
  - `atomic.add`: Require exactly 2 arguments (`[i32, i32]`).
  - `atomic.cas`: Require exactly 3 arguments (`[i32, i32, i32]`).
  - `atomic.lock` / `atomic.unlock`: Require exactly 1 argument (`[i32]`).
- [ ] **Patch `src/checker.rs` `thread.join`:** Enforce `Type::I32` on operand 0 (`if t != Type::I32 { return Err(...); }`).
- [ ] **Synchronize `sys.print` Typing:** Align `src/checker.rs:574` with `src/compiler/wasm.rs:908` by either restricting `sys.print` to `Type::Str` in the checker or implementing value formatting for primitives in the WASM backend.
- [ ] **Fix `mem.free` Parity:** Add `mem_free` to the `Kw` enum and keyword table in `aipl_src/codegen.aipl` as a no-op, preventing self-host error 987.

### Priority 1: Semantic Parity & Safety Alignment (Next Sprint)
- [ ] **Lower Contracts to WebAssembly Traps:** Modify `src/compiler/wasm.rs` and `aipl_src/codegen.aipl` to emit WASM conditional branches for `req` and `ens` that execute `unreachable` on contract failure.
- [ ] **Emit Array Bounds Traps in Codegen:** Emit an explicit length check (`local.get index; local.get arr; i32.load (len offset); i32.ge_u; if unreachable end`) in compiled backends for `arr.get` and `arr.set`.
- [ ] **Remove or Implement Floating-Point Memory Ops:** Either implement `f32.load`, `f64.load`, `f32.store`, and `f64.store` in the VM and WASM backends, or formally excise `mem.load_f32/f64` from the grammar and specification.
- [ ] **Eliminate 1 GiB Stack Hack:** Refactor `src/resolver.rs` and `src/parser.rs` to use iterative loops or an explicit heap-allocated worklist instead of unbounded recursion.

### Priority 2: Infrastructure & Harness Hygiene (Follow-up)
- [ ] **Fix Hollow Test Assertion:** Rewrite `tests/test_opcode_conformance.rs:181` to assert that both backends succeed with equivalent results, or explicitly document an approved skip list for unsupported opcodes.
- [ ] **Clean `Cargo.toml` Build Targets:** Remove the redundant `[[bin]]` declaration for `benches/execution_bench.rs` to silence manifest warnings.
- [ ] **Validate Generic Templates at Parse Time:** Perform structural type checking on generic template bodies before instantiation so syntax/type errors in uncalled generics are caught during compilation.


---

## Response (2026-10-05, branch `features/audit-fixes`)

Each finding was reproduced against the current code (which also has the unions, unsigned comparisons, and checked arithmetic added after 7c67971) before anything was changed. Then every operator was run with 0 to 3 operands of each kind of value (about 2,800 programs), plus 42 other forms in statement position, looking for anything `aipl verify` accepts but a backend cannot run or compile. That sweep found more than the audit did.

| Finding | Outcome |
|---|---|
| P0 `(atomic.add)` and the other atomics: no operand checks; the compilers panic | Fixed. All four atomics check operand count and types (`atomic.cas takes 3 operands, (atomic.cas p expected new); got 2`) |
| P0 `thread.join` accepts any type | Fixed: the handle must be `i32` |
| P0 `sys.print` accepted any type, but the compilers take only `str` | Fixed in the checker: `str` only, with the `io.print_*` alternatives in the message |
| P0 `mem.free` breaks self-hosted parity | Fixed by removing `mem.free`: a no-op free in a bump allocator promises something that never happens. The parser explains, pointing to `std/arena` |
| P1 float memory ops in the grammar, rejected everywhere | Removed from the language (parser error with the alternatives: struct fields, `(arr f64)`, or `mem.load64` plus `f64.reinterpret_i64`) |
| P2 conformance test passes when either backend fails | Rewritten, and it was worse than reported: it also skipped ops that failed to parse or check. Every op must now check, run in the VM, compile to wasm that validates in value and statement position, and match the self-hosted compiler's bytes. Mutation-checked |
| P2 `Cargo.toml` warning | Fixed: `benches/execution_bench.rs` (a VM loop timing and a token-count banner from the initial commit) deleted; the real benchmarks are `benchmarks/` and `tools/bench.py` |
| P1 contracts and bounds checks only in the VM | Accurate and already documented (AIPL_SPEC.md 7.6, 4.E; LANGUAGE_GAPS.md). Compiling them is the roadmap's next step after the AIPL checker (PROGRESS.md) |
| P1 1 GiB stack | Reserved address space for the recursive interpreter, not memory used. A design trade-off, not a vulnerability; retiring the VM (audit S4) removes it |
| P2 generic templates checked only through instances | Accurate and documented (LANGUAGE_GAPS.md 2) |

**Found by the sweep, not by the audit:**
- Arithmetic on `bool` and `str`, integer-only ops (`%`, shifts, bitwise, `divu`, `remu`) on floats, and ordering (`lt` ...) on `bool` and `str` all passed `verify` and failed to compile. The checker now rejects them. `(+ str str)`, which only ever worked in the VM, is gone; text is built with `std/buf`. The self-test suite's own `report` used it (it now compiles to wasm too).
- **A miscompilation in the Rust backend:** `checked.add/sub/mul`, `sys.time`, `sys.monotonic`, `sys.random`, `thread.spawn`, `thread.join`, `atomic.add`, and `atomic.cas`, used as a statement (value discarded), produced invalid wasm: their value was never dropped. The backend's list of value-producing ops had drifted from its type table; it now asks the type table, so a new op cannot be missed. The self-hosted compiler was right all along, and byte parity is what exposed the difference.

After the fixes the sweep finds no program that `verify` accepts and a backend rejects.
