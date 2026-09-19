# AIPL Formal Specification (v2.0 Systems & Concurrency Edition)
## AI Programming Language: Machine-Native Formal Specification

> **Integer semantics: wasm semantics are the spec.** `i32` and `i64` are wrapping two's-complement; the VM must match wasmtime bit-for-bit, and any divergence is a VM bug. Enforced by `tests/test_differential.rs`, which runs every case in both backends.

AIPL (AI Programming Language) is an ultra-dense, non-ambiguous, formally verifiable systems programming language, self-hosting compiler, and Intermediate Representation (IR) designed exclusively for AI agent consumption, high-performance WebAssembly compilation, linear memory manipulation, atomic swarm concurrency, and native vector embedding RAG database operations.

---

## 1. Syntax Architecture: Dual Representation

AIPL has two canonical forms:
1. **S-Expression Canonical Text Representation (`.aipl`)**: Context-free, parenthesis-delimited AST. Eliminates human syntactic ambiguities (no operator precedence rules, no indentation semantics, no semicolon requirements).
2. **Compact Binary AST Payload (`.baipl`)**: 1-byte opcode encoded MessagePack binary representation used for instant zero-parse serialization between AI swarms over HTTP, gRPC, or IPC.

---

## 2. Updated Formal Grammar (EBNF)

```ebnf
program        ::= "(" "module" identifier import* fn_def* ")" ;
import         ::= "(" "import" identifier [ "as" identifier ] ")" ;

fn_def         ::= "(" "fn" identifier "[" param* "]" "->" type contract* expr* ")" ;
param          ::= identifier ":" type ;
contract       ::= "(" ("req" | "ens" | "inv") expr ")" ;

let_stmt       ::= "(" "let" identifier ":" type expr ")" ;
set_stmt       ::= "(" "set!" identifier expr ")" ;

type           ::= "i32" | "i64" | "f32" | "f64" | "bool" | "str" | "void"
                 | "(" "ptr" type ")"
                 | "(" "arr" type integer ")"
                 | "(" "vec" type integer ")"
                 | "(" "fn" "(" type* ")" "->" type ")" ;

expr           ::= literal
                 | identifier
                 | "(" "if" expr expr expr ")"
                 | "(" "loop" identifier expr expr expr expr* ")"
                 | "(" "while" expr expr* ")"
                 | "(" "call" identifier expr* ")"
                 | "(" "block" expr* ")"
                 | "(" "ok" expr ")"
                 | "(" "err" expr ")"
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" op expr* ")" ;

op             ::= arithmetic_op | bitwise_op | memory_op | atomic_op | comp_op | array_op
                 | sys_op | fs_op | thread_op ;

arithmetic_op  ::= "+" | "-" | "*" | "/" | "%" | "divu" | "remu" ;
bitwise_op     ::= "^" | "shl" | "shr" | "shru" | "bitand" | "bitor" ;
memory_op      ::= "mem.load8" | "mem.load32" | "mem.load64" | "mem.load_f32" | "mem.load_f64"
                 | "mem.store8" | "mem.store32" | "mem.store64" | "mem.store_f32" | "mem.store_f64"
                 | "mem.alloc" | "mem.free" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "and" | "or" | "not" ;
array_op       ::= "arr.get" | "arr.set" ;
sys_op         ::= "sys.print" | "sys.time" | "sys.exit" ;
fs_op          ::= "fs.open" | "fs.read" | "fs.write" | "fs.close" | "fs.delete" ;
thread_op      ::= "thread.spawn" | "thread.join" ;
```

Notes on the grammar as implemented by `src/parser.rs`:
- A module body is imports followed by function definitions only. `let` and `set!` are expressions that appear inside function bodies, not at module level.
- Comments start with `;;` and run to end of line.
- Integer literals are decimal, optionally negative (`-1` is one token) and are `i32`. An `i64` literal carries the suffix as part of the token: `42i64`, `-7i64`. Float literals must contain a `.` and at least one digit (`1.0`, not `1` or `inf`) and are `f64`. Strings are double-quoted. Booleans are `true` / `false`.
- `(call f ...)` takes a bare function name, never an expression. Imported functions are called as `(call modname.fn ...)`.
- `(ptr T)` and `(fn ...)` types exist in the AST but are not yet parseable. `i64` is fully supported (section 8.1).
- Conversion ops (not in the op families above): `(i64.extend_s x)`, `(i64.extend_u x)`, `(i32.wrap x)`.

---

## 3. Type System & Formal Contracts

AIPL is strongly and statically typed. Type inference is supported, but explicit annotations are preferred for zero-ambiguity LLM generation.

### Scalar Primitives
- `i32`, `i64`: 32-bit and 64-bit signed integers.
- `f32`, `f64`: 32-bit and 64-bit IEEE-754 floating point numbers.
- `bool`: Logical `true` / `false`.
- `str`: Immutable UTF-8 string pointer + length.

### Raw Pointers & Compound Types
- `(ptr T)`: Raw memory address pointer to type `T` in WebAssembly linear memory.
- `(arr T N)`: Fixed-length array of type `T` and length `N`.
- `(vec T N)`: SIMD-aligned vector payload of length `N` for AI embeddings.

### Formal Verification Contracts
Functions support formal pre-conditions and post-conditions evaluated statically by the AIPL verifier before compilation:
```lisp
(fn db_read_slot [ptr:i32 offset:i32] -> i32
  (req (gt ptr 0))
  (req (gte offset 0))
  (ens (gte res 0))
  (mem.load32 (+ ptr offset)))
```

---

## 4. Systems Operations (Memory & Atomics)

### A. Raw WebAssembly Linear Memory Loads & Stores
- `(mem.load32 ptr)` -> Reads 4 bytes from linear memory offset `ptr` (`i32.load`).
- `(mem.store32 ptr val)` -> Writes 4 bytes to linear memory offset `ptr` (`i32.store`).
- `(mem.alloc size)` -> Dynamic heap allocation tracking.

### B. Atomic Swarm Synchronization
- `(atomic.lock mutex_ptr)` -> Acquires thread-safe mutex lock.
- `(atomic.unlock mutex_ptr)` -> Releases mutex lock.
- `(atomic.add ptr val)` -> Atomic memory addition.

---

## 5. WebAssembly Binary Execution Semantics

AIPL code maps 1-to-1 to WebAssembly binary opcodes:
- `(mem.load32 ptr)` -> `i32.load (MemArg { offset: 0, align: 2 })`
- `(mem.store32 ptr val)` -> `i32.store (MemArg { offset: 0, align: 2 })`
- `(shl a b)` -> `i32.shl`
- `(shr a b)` -> `i32.shr_s`
- `(bitand a b)` -> `i32.and`
- `(bitor a b)` -> `i32.or`

---

## 6. Toolchain Pipeline & Compiler Expectations

Every AIPL entry point runs the same four stages in order. A failure at any stage aborts with a single diagnostic string and a non-zero exit code; nothing downstream runs.

```
source.aipl
   │
   ▼
[1] Resolver   (src/resolver.rs)   reads the file, parses it, follows every (import ...),
   │                               renames imported fns to `mod.fn`, returns ONE flat Module
   ▼
[2] Parser     (src/parser.rs)     S-expression text -> AST (Module { name, imports, functions })
   │                               every node carries a (line, col) span
   ▼
[3] Checker    (src/checker.rs)    static types + contract typing; no inference across fns
   │
   ├──────────────────────────────┐
   ▼                              ▼
[4a] VM        (src/vm.rs)       [4b] Wasm backend (src/compiler/wasm.rs)
     tree-walking interpreter         emits a core wasm module via wasm-encoder
     runs contracts at call time      contracts are NOT emitted (checked statically only)
```

### 6.1 CLI surface (`src/main.rs`)

| Command | Stages run | Success output |
|---|---|---|
| `aipl verify FILE` | 1, 2, 3 | `[AIPL Verifier] SUCCESS: Module 'NAME' is 100% type-safe and contracts verified!` |
| `aipl eval FILE [--func NAME]` | 1, 2, 3, 4a | `[AIPL Result]: Int(42)` (Rust `Debug` of the returned `Value`; default `--func main`) |
| `aipl compile FILE [-o out.wasm]` | 1, 2, 3, 4b | `[AIPL Compiler] Successfully compiled 'FILE' -> 'out.wasm' (N bytes)` |
| `aipl test FILE [--func run_all]` | 1, 2, 3, 4a | `[AIPL Test] All groups passed.` and exit 0; otherwise `N group(s) failed.` and exit 1 |
| `aipl binary-encode FILE [-o out.baipl]` | 2 only | MessagePack encoding of the AST |
| `aipl binary-decode FILE` | none | lists function names in a `.baipl` |
| `aipl serve [--addr 127.0.0.1:8080]` | on request | agent RPC server |

`eval` and `test` only invoke zero-argument functions. To exercise a function that takes parameters, wrap it in a zero-arg driver or write a Rust test (section 10.2).

### 6.2 What a compiled `.wasm` module looks like

`WasmCompiler::compile` produces a core WebAssembly 1.0 module with exactly these sections, in this order: **type, function, memory, global, export, code**. There is no import section, no start function, and no data segments (string literals compile to `i32.const 0` and are meaningless in wasm today).

| Item | Value |
|---|---|
| Memory | one linear memory, min 1 page (64 KiB), max 100 pages, exported as `"memory"` |
| Global 0 | mutable `i32`, initialised to `1024`; this is the `mem.alloc` bump pointer |
| Exports | **every** function in the flat module, exported under its AIPL name (`add`, `compiler.tokenize`, ...) |
| Function types | params map `i32/bool/str/void -> i32`, `f32 -> f32`, `f64 -> f64`; a `void` return is an empty result list |
| Locals | every `let` anywhere in the body (including nested in `if`/`while`/`loop`/`block`) plus every `loop` induction variable becomes one wasm local, allocated after the params |

Bytes 0..8 are always `00 61 73 6D 01 00 00 00` (`\0asm`, version 1). A module that compiles must also pass `wasmparser::Validator::validate_all`; the test suite enforces this.

### 6.3 Backend support matrix (as of 2026-09-18)

"Yes" means the op runs. "Err" means the backend returns an explicit error naming the op; there are no silent defaults or no-ops in either backend.

| Ops | Checker | VM | Wasm |
|---|---|---|---|
| `+ - * / % divu remu ^ shl shr shru bitand bitor` on `i32` / `i64` | Yes | Yes, wrapping at the operand width | Yes (`i32.*` / `i64.*`) |
| `+ - * /` on `f64` | Yes | Yes | Yes (`f64.*`); `%`, `divu`, `remu`, shifts, bitwise on floats are Err |
| `eq neq lt lte gt gte` on `i32` / `i64` / `f64` / `bool`; `and or not` | Yes | Yes | Yes (`i32.*` / `i64.*` / `f64.*`) |
| `i64.extend_s i64.extend_u i32.wrap` | Yes | Yes | Yes |
| `mem.load8/32/64`, `mem.store8/32/64` | Yes | Yes (1 MiB memory) | Yes |
| `mem.load_f32/f64`, `mem.store_f32/f64` | Yes | Err | Err |
| `mem.alloc` | Yes | Yes, bump from 1024, never frees | Yes, bump via global 0 |
| `mem.free` | Yes | no-op, returns void | Err |
| `atomic.add/cas/lock/unlock` | Yes | Yes, real across OS threads | Err (needs shared memory) |
| `arr.get / arr.set` | Yes | Err | Err |
| `sys.print` | Yes | Yes (`println!`) | Err (needs WASI, P6) |
| `sys.time / sys.exit` | Yes | Err | Err |
| `fs.open/read/write/close/delete` | Yes | Yes, real `std::fs` | Err (needs WASI, P6) |
| `thread.spawn / thread.join` | Yes | Yes, real `std::thread` | Err (needs wasi-threads) |
| `ok / err` | Yes | Yes | compiles to the bare payload (no tag) |
| `match_result` | Yes | Yes | Err (`Wasm Codegen: MatchResult is not supported in the wasm backend`) |
| `str` literals and `(+ str str)` | Yes | Yes | compiles but yields `0`; **VM-only in practice** |

Rule of thumb for code generators: anything in the `sys.*`, `fs.*`, `thread.*`, `atomic.*`, or `arr.*` families, `match_result`, and anything touching `str`, is **VM-only** today. Pure integer/boolean code with `mem.load/store` and `mem.alloc` runs identically in both.

---

## 7. Typing and Evaluation Rules by Example

The checker is `src/checker.rs`; the VM is `src/vm.rs`. Both agree on these rules.

### 7.1 Every form has a type, including statements

| Form | Type | Example |
|---|---|---|
| integer literal | `i32` | `42`, `-7` |
| suffixed integer literal | `i64` | `42i64`, `-7i64`, `4294967296i64` |
| float literal | `f64` (never `f32`) | `3.5` |
| `true` / `false` | `bool` | |
| `"text"` | `str` | |
| `(let x:T v)` | `T` (the declared type; `v` must be exactly `T`) | `(let n:i32 (+ 1 2))` |
| `(set! x v)` | type of `x` (`v` must match; `x` must already be declared) | |
| `(if c a b)` | type of `a`, which must equal type of `b`; `c` must be `bool` | |
| `(loop i s e st body*)` | `void`; `s e st` must be `i32`; `i` is visible only in the body | |
| `(while c body*)` | `void` | |
| `(block e1 ... en)` | type of `en` (or `void` if empty) | |
| `(call f args)` | declared return type of `f`; arity and every arg type must match exactly | |
| `(ok v)` | `Result<typeof v, i32>` | |
| `(err e)` | `Result<i32, typeof e>` | |
| `(match_result r (ok v body*) (err e body*))` | type of the last expr of the **ok** body; `v` and `e` are bound as `i32` | |
| binary arithmetic / bitwise | type of the operands, which must be equal | `(+ 1 2)` is `i32`; `(+ 1.0 2.0)` is `f64`; `(+ 1 2.0)` is an error |
| comparisons | `bool`; operands must have equal type | |

A function body is a sequence of expressions. The **last** expression's type must equal the declared return type unless the return type is `void`, in which case the last value is discarded.

### 7.2 Valid: value-producing `if` as the final expression

```lisp
(module ex_if
  (fn sign [n:i32] -> i32
    (if (lt n 0) -1
        (if (eq n 0) 0 1))))
```
`aipl eval` on a driver calling `(call sign -5)` prints `[AIPL Result]: Int(-1)`. Compiles to `if (result i32) ... else ... end` in wasm.

### 7.3 Invalid: mixed branch types

```lisp
(fn bad [n:i32] -> i32
  (if (lt n 0)
      (set! n 0)      ;; type i32 (type of n)
      (while false))  ;; type void
  n)
```
Checker output: `3:5: If branch type mismatch: then is I32, else is Void` (position of the `(if`).

### 7.4 The mutate-then-yield idiom

Because `set!` has the variable's type rather than `void`, an `if` whose branches only mutate state type-checks but is awkward to use as the final expression of an `i32` function. The established idiom is to wrap side effects in `block` and end with the value you mean:

```lisp
(fn clamp_to_100 [n:i32] -> i32
  (let out:i32 n)
  (if (gt n 100)
      (block (set! out 100) out)
      out))
```
Prefer this over relying on the `if`'s own value when a branch has side effects. (P7 in `AIPL_Structural_Audit.md` will make `set!` and `let` void and void-`if` legal, which removes the need for this idiom.)

### 7.5 Loops

`loop` is **inclusive** of its end bound and always steps by the third argument:

```lisp
(fn sum_to_ten [] -> i32
  (let acc:i32 0)
  (loop i 1 10 1
    (set! acc (+ acc i)))
  acc)          ;; => 55, because the bound is inclusive: 1+2+...+10
```
`(loop i 0 9 1 ...)` runs 10 times. `(loop i 0 0 1 ...)` runs once. `while` is the only loop with an arbitrary exit condition; there is no `break`, `continue`, or `return` (P11).

```lisp
(fn count_down [start:i32] -> i32
  (req (gte start 0))
  (let n:i32 start)
  (let steps:i32 0)
  (while (gt n 0)
    (set! n (- n 1))
    (set! steps (+ steps 1)))
  steps)
```

### 7.6 Contracts: what runs, where, and what happens on failure

- `req` expressions are type-checked with the parameters in scope and **evaluated by the VM before the body**. Every `req` must be `bool`.
- `ens` expressions are type-checked with parameters plus `res` (bound to the return type) and **evaluated by the VM after the body** with `res` bound to the actual result.
- `inv` is type-checked like `req` but is not evaluated at runtime by any backend today.
- The wasm backend does **not** emit contracts. A compiled module has no runtime checks; the contracts were only verified to be well-typed, not proven.

```lisp
(module contracts_demo
  (fn safe_div [num:i32 den:i32] -> i32
    (req (neq den 0))
    (ens (or (eq num 0) (neq res 0)))   ;; weak, but bool
    (/ num den))

  (fn main [] -> i32
    (call safe_div 10 0)))
```
`aipl eval contracts_demo.aipl` fails with `Pre-condition (req <expr>) failed in function 'safe_div'`, where `<expr>` is currently the Rust `Debug` dump of the contract AST (`Op { op: Neq, args: [...] }`), not source text. A `req` or `ens` that is not `bool` is rejected at check time: `L:C: Contract expression in 'safe_div' must evaluate to Bool, got I32`.

### 7.7 Results

`ok`/`err` build a result value; `match_result` consumes one. The payloads are bound as `i32` in both arms.

```lisp
(fn parse_digit [c:i32] -> i32
  (let r:i32
    (match_result
      (if (and (gte c 48) (lte c 57)) (ok (- c 48)) (err -1))
      (ok v v)
      (err e 0)))
  r)
```
Note the `if` here has branches of type `Result<i32,i32>` and `Result<i32,i32>`: `(ok x)` is `Result<i32, i32>` and `(err -1)` is `Result<i32, i32>`, so they agree. Mixing `(ok 1.0)` with `(err 0)` would not. `match_result` is **VM-only**: the wasm backend rejects it, and `ok`/`err` compile to their bare payload with no tag, so a compiled module cannot distinguish the two arms.

### 7.8 Memory

Linear memory is byte-addressed. The VM has 1 MiB; wasm starts at 64 KiB and may grow to 100 pages. Nothing below address 1024 is handed out by `mem.alloc`, so the range `0..1024` is conventionally scratch space in examples and tests. Loads and stores are little-endian, unaligned access is allowed, and out-of-bounds access is a VM runtime error (`Memory store out of bounds: ptr N`) and a wasm trap.

```lisp
(fn pack_two [] -> i32
  (let p:i32 (mem.alloc 8))        ;; p == 1024 on a fresh VM or module
  (mem.store32 p 7)
  (mem.store32 (+ p 4) 35)
  (+ (mem.load32 p) (mem.load32 (+ p 4))))   ;; 42
```

---

## 8. Integer Semantics

`i32` is a wrapping two's-complement 32-bit integer in **both** backends; the wasm instruction set defines the behaviour and the VM must match it bit for bit. Internally the VM stores integers in an `i64` but truncates every operand to `i32` before each operation, so no value wider than 32 bits is observable.

| Expression | Result | Why |
|---|---|---|
| `(+ 2147483647 1)` | `-2147483648` | wraps |
| `(* 65536 65536)` | `0` | wraps |
| `(- 0 -2147483648)` | `-2147483648` | wraps |
| `(/ -7 2)` | `-3` | truncates toward zero (`i32.div_s`) |
| `(% -7 2)` | `-1` | sign follows the dividend (`i32.rem_s`) |
| `(/ 7 0)` | runtime error | VM: `Division by zero`; wasm: trap |
| `(/ -2147483648 -1)` | runtime error | VM: `Integer overflow`; wasm: trap |
| `(% -2147483648 -1)` | `0` | defined, not a trap (`i32.rem_s`) |
| `(divu -1 2)` | `2147483647` | operands reinterpreted as unsigned |
| `(remu -1 2)` | `1` | |
| `(shl 1 33)` | `2` | shift count masked to 5 bits (`33 & 31 == 1`) |
| `(shr -8 1)` | `-4` | arithmetic shift, sign-extending |
| `(shru -8 1)` | `2147483644` | logical shift, zero-filling |

### 8.1 `i64`

`i64` is a first-class declarable type with the same wrapping two's-complement semantics at 64 bits. There is **no implicit widening**: an `i32` and an `i64` never meet in one operation, and the checker rejects `(+ 1 2i64)` with `Type mismatch in binary op: I32 vs I64`. Conversions are explicit ops that map one-to-one onto wasm instructions.

| Form | Meaning | wasm |
|---|---|---|
| `42i64`, `-7i64`, `4294967296i64` | 64-bit literal; the suffix is part of the token | `i64.const` |
| `(let x:i64 0i64)`, `[a:i64]`, `-> i64` | declarations | local/param/result `i64` |
| `+ - * / % divu remu ^ bitand bitor shl shr shru` on two `i64` | 64-bit wrapping; shift counts masked to 6 bits | `i64.add` ... `i64.shr_u` |
| `eq neq lt lte gt gte` on two `i64` | signed comparison, yields `bool` | `i64.eq` ... `i64.ge_s` |
| `(i64.extend_s x)` | `i32 -> i64`, sign-extending | `i64.extend_i32_s` |
| `(i64.extend_u x)` | `i32 -> i64`, zero-extending | `i64.extend_i32_u` |
| `(i32.wrap x)` | `i64 -> i32`, low 32 bits | `i32.wrap_i64` |
| `(mem.load64 p)` | reads 8 little-endian bytes as `i64` | `i64.load` |
| `(mem.store64 p v)` | `v` must be `i64` | `i64.store` |

| Expression | Result |
|---|---|
| `(+ 2147483647i64 1i64)` | `2147483648` (no i32 wrap) |
| `(+ 9223372036854775807i64 1i64)` | `-9223372036854775808` |
| `(* 4294967296i64 2147483648i64)` | `-9223372036854775808` |
| `(divu -1i64 2i64)` | `9223372036854775807` |
| `(shl 1i64 65i64)` | `2` (count masked: `65 & 63 == 1`) |
| `(i64.extend_s -1)` | `-1` |
| `(i64.extend_u -1)` | `4294967295` |
| `(i32.wrap 4294967301i64)` | `5` |
| `(i32.wrap (+ (i64.extend_s 2147483647) 1i64))` | `-2147483648` |

Loop bounds, memory addresses, `mem.alloc` sizes, file descriptors, and thread handles remain `i32`. To index memory with an `i64` computation, narrow it first with `i32.wrap`. The VM prints an `i64` result as `Int64(n)`, an `i32` as `Int(n)`.

### 8.2 Type-directed code generation

The wasm backend selects instructions from the static operand type (`i32` / `i64` / `f32` / `f64`), and an `if` whose branches are `i64` or `f64` gets a matching block result type. `f64` arithmetic (`+ - * /`) and comparisons therefore compile and validate. `%`, `divu`, `remu`, shifts, and bitwise ops on floats are rejected at compile time with `Wasm Codegen: <op> is not supported for operands of type F64`.

**Status:** the wrapping rules above for both widths are implemented in `src/vm.rs`, covered by `tests/test_i64.rs` (VM result plus wasm validation), and proven equal across backends by `tests/test_differential.rs`, which executes every case in this section in both the VM and wasmtime and asserts identical results. When the two disagree, wasm is right and the VM is fixed (section 10.4).

---

## 9. Diagnostics

Every parser and checker error is a single line of the form

```
<line>:<col>: <message>
```

with 1-based line and column of the offending token or the opening `(` of the offending form. The resolver prefixes the file path: `aipl_src/memory.aipl: 12:5: Undefined variable 'foo'`. VM runtime errors (contract failures, division by zero, out-of-bounds memory, unknown thread handle) currently have **no** position.

Representative messages, exactly as produced:

| Situation | Message |
|---|---|
| missing `)` at end of file | `2:27: Expected RParen, got EOF` |
| extra `)` after the module | `3:1: unexpected tokens after module end — check for an extra ')'` |
| unknown operator | `3:5: Unknown op/keyword: badop` |
| unterminated string | `4:12: Unterminated string literal` |
| `i64` in a type position | `2:20: i64 type is unsupported` |
| `(ptr i32)` in a type position | `2:20: Unknown compound type: ptr` |
| wrong literal type in `let` | `3:5: Type mismatch in 'let': expected I32, got Bool` |
| undefined name | `3:5: Undefined variable 'x'` |
| `set!` before `let` | `3:5: Undefined variable 'x' in set!` |
| `if` branches disagree | `3:5: If branch type mismatch: then is I32, else is Bool` |
| non-bool `if` condition | `3:5: If condition must be Bool, got I32` |
| wrong arity | `4:5: Function 'add' expects 2 arguments, got 1` |
| wrong argument type | `4:5: Arg 1 of 'add' expects I32, got Bool` |
| body/return mismatch | `2:3: Function 'f' expects return type I32, but body returned Bool` |
| unsupported op in a backend | `Wasm Codegen: SysTime is not supported in the wasm backend` or `SysTime not supported in VM backend: system ops not implemented` |

Type names in messages are the Rust `Debug` spelling: `I32`, `F64`, `Bool`, `Str`, `Void`, `ResultType(I32, I32)`.

---

## 10. Testing Conventions

There are three layers of tests. Each one must fail loudly on fabricated work; a test that only asserts "returned a number" or "produced more than N bytes" is not acceptable in this repository (see `AIPL_Structural_Audit.md`, item U6).

### 10.1 AIPL-native self-tests (`aipl test`)

Each real module in `aipl_src/` exposes a zero-arg `run_<module>_tests` function that returns an `i32` count of passing sub-tests (or `1` for a single pass / `0` for fail). The suite entry point `aipl_src/test_suite.aipl` imports every verified module, calls each runner, compares against the expected count, prints one `[PASS]`/`[FAIL]` line per group with `sys.print`, and returns the number of failing groups. The Rust `test` subcommand turns that into the exit code.

```lisp
;; aipl_src/test_suite.aipl (abridged)
(module test_suite
  (import compiler)
  (import memory)

  (fn report [label:str result:i32 expected:i32] -> i32
    (if (eq result expected)
        (block (sys.print (+ "[PASS] " label)) 0)
        (block (sys.print (+ "[FAIL] " label)) 1)))

  (fn run_all [] -> i32
    (let failures:i32 0)
    (set! failures (+ failures (call report "compiler: tokenizer (2 tests)" (call compiler.run_tokenizer_tests) 2)))
    (set! failures (+ failures (call report "memory: allocator + arena"     (call memory.run_memory_tests)     1)))
    failures))
```

Pattern for a module's own runner: perform real work, then assert on the **content** of the result, not its shape.

```lisp
;; aipl_src/memory.aipl
(fn run_memory_tests [] -> i32
  (let p1:i32 (call aipl_heap_alloc 64))
  (let p2:i32 (call aipl_heap_alloc 128))
  (let arena:i32 (call arena_create 1024))
  (let a1:i32 (call arena_alloc arena 32))
  (let a2:i32 (call arena_alloc arena 64))
  (let _r:i32 (call arena_reset arena))
  (let a3:i32 (call arena_alloc arena 32))
  (if (and (gt p2 p1) (and (gt a2 a1) (eq a3 a1))) 1 0))
```

Expected terminal output:
```
[AIPL Test] Running 'run_all' from 'aipl_src/test_suite.aipl'...

[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] memory: allocator + arena
[PASS] file_io: real disk round-trip

[AIPL Test] All groups passed.
```

Rules: import a module into `test_suite.aipl` only once its runner is verified to do real work. Modules whose tests depend on `thread.spawn` must be run standalone (`aipl test aipl_src/thread_sync.aipl --func run_thread_tests`) because the resolver renames functions and `thread.spawn` looks its target up by a name stored in memory (section 11).

### 10.2 Rust integration tests (`cargo test`, `tests/*.rs`)

Rust tests drive the library crate `aipl_core` directly and are the only way to call AIPL functions with arguments. The canonical shape exercises **every** stage and asserts concrete values:

```rust
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmparser::Validator;

#[test]
fn fib_runs_and_compiles() {
    let src = r#"
    (module compute
      (fn fib [n:i32] -> i32
        (if (lte n 1) n
            (+ (call fib (- n 1)) (call fib (- n 2))))))
    "#;
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");

    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("fib", vec![Value::Int(7)]).unwrap(), Value::Int(13));

    let wasm = WasmCompiler::compile(&module).expect("compile");
    assert!(wasm.starts_with(&[0x00, 0x61, 0x73, 0x6D]));
    Validator::new().validate_all(&wasm).expect("wasm must validate");
}
```

To assert a diagnostic, match on the exact `L:C:` prefix so a regression in position tracking fails the test:

```rust
let err = Parser::parse("(module m\n  (fn f [] -> i32 42)").unwrap_err();
assert!(err.starts_with("2:21:"), "got {err}");
```

To assert that a backend **rejects** something rather than faking it:

```rust
let module = Parser::parse("(module m (fn f [] -> void (sys.print \"x\")))").unwrap();
let err = WasmCompiler::compile(&module).unwrap_err();
assert!(err.contains("sys.print not supported in wasm backend"));
```

Files today: `tests/test_all.rs` (pipeline smoke), `tests/test_v2.rs` (memory, atomics across real threads, real file I/O, results, imports), `tests/test_diagnostics.rs` (exact `L:C:` prefixes), `tests/test_i64.rs` (64-bit type, VM plus wasm validation), `tests/test_opcode_conformance.rs` (10.3), `tests/test_differential.rs` (10.4).

### 10.3 Opcode conformance contract

`tests/test_opcode_conformance.rs` holds a minimal program for every `OpCode` variant. The `match` over `OpCode` has no wildcard arm, so adding a variant to `src/ast.rs` without a program is a compile error. For each variant the test asserts exactly one of:

- **(a)** the VM returns `Ok`, the wasm backend returns bytes, and `wasmparser` validates them; or
- **(b)** the checker, the VM, or the wasm backend returns `Err`.

An op that "succeeds" by returning a default value in one backend and a no-op in the other satisfies neither and fails the suite. New opcodes must be added to this file in the same commit that adds them to the AST.

### 10.4 Differential testing (`tests/test_differential.rs`)

This is what makes "wasm semantics are the spec" enforceable. `wasmtime` is a dev-dependency; the harness compiles a module with `WasmCompiler`, instantiates the bytes in wasmtime (no imports are needed), and calls the export. `differential(module, wasm, fn_name, args)` runs the same call in a fresh `VM` and a fresh wasmtime instance and asserts one of:

- both return the same value (`bool` is normalised to `Int(0|1)`, its wasm shape), or
- both fail (VM `Err` and wasmtime trap, e.g. division by zero).

A VM `req`/`ens` failure paired with a wasmtime success is **not** a divergence, because the wasm backend emits no contracts. Any other mismatch panics with `DIVERGENCE ... (fix src/vm.rs)`. The wasm side is never changed to match the interpreter.

Coverage:

- Every edge case in section 8 and 8.1 as a single-expression program.
- Control flow: inclusive `loop` bound, stepped and negative-start loops, `while` with `set!`, nested `if`/`block` values, recursion, memory round trips through the bump allocator.
- Every `examples/*.aipl`: resolved, checked, compiled; every function returning `i32`/`i64`/`bool` with all-`i32` params is called over nine fixed argument tuples in both backends. Files the wasm backend rejects are skipped with a printed reason. `hello_browser.aipl` is pinned as stale (removed `dom.*` ops) and the test fails if it ever parses again without being unpinned. The test asserts at least 4 files and 40 calls were compared so it cannot silently go vacuous.
- `aipl_src/codegen.aipl` is pinned as not wasm-compilable (`test_compile_*` use `fs.*`); the pin flips to a real comparison of `test_signatures_and_locals` the day the module compiles.

Sample argument values are deliberately small. Example functions use parameters as loop bounds, and a tree-walking VM asked to iterate `i32::MAX` times is not a test, it is a hang. Wrap-around is covered by the explicit expression cases instead.

---

## 11. Imports and Multi-Module Programs

```lisp
;; util.aipl
(module util
  (fn double [x:i32] -> i32 (* x 2)))

;; main.aipl
(module app
  (import util)
  (fn main [] -> i32 (call util.double 21)))      ;; => Int(42)
```

- `(import name)` finds `name.aipl` next to the importing file (the resolver also tries the entry file's directory), parses it, and merges its functions into the entry module renamed as `name.fn`. `(import name as u)` lets you write `(call u.double ...)` locally; it is rewritten to `util.double` before checking.
- Import depth is flattened to one level: a function from a module imported by an import is still `directimport.fn`, not `a.b.fn`. Diamond imports produce one copy. Cycles are an error naming the file.
- The entry module's own functions keep bare names. In a compiled `.wasm`, exports are `main` and `util.double`.
- **Caveat:** `thread.spawn` names its target by a byte string in linear memory, which the resolver cannot see. A module that spawns `worker` will fail with `thread.spawn: unknown function 'worker'` once imported, because the real function is now `mod.worker`. Run such modules as the entry file.

---

## 12. Worked Examples

Each example is complete and runs with the command shown. Expected output is what the current toolchain prints.

### 12.1 Pure computation, both backends

```lisp
;; gcd.aipl
(module gcd_demo
  (fn gcd [a:i32 b:i32] -> i32
    (req (and (gt a 0) (gt b 0)))
    (ens (gt res 0))
    (let x:i32 a)
    (let y:i32 b)
    (while (neq y 0)
      (let t:i32 y)
      (set! y (% x y))
      (set! x t))
    x)

  (fn main [] -> i32 (call gcd 1071 462)))
```
```
$ aipl eval gcd.aipl
[AIPL VM] Executing function 'main' from 'gcd.aipl'...
[AIPL Result]: Int(21)
$ aipl compile gcd.aipl -o gcd.wasm
[AIPL Compiler] Successfully compiled 'gcd.aipl' -> 'gcd.wasm' (N bytes)
```
The `.wasm` exports `gcd`, `main`, and `memory`. Note the `let t` inside the `while` body: it becomes a wasm local of the function and is re-assigned each iteration, which is the intended semantics.

### 12.2 Memory as a data structure, both backends

```lisp
(module stack_demo
  ;; stack layout: [count:i32][slot0:i32][slot1:i32]...
  (fn stack_new [cap:i32] -> i32
    (let s:i32 (mem.alloc (+ 4 (* cap 4))))
    (mem.store32 s 0)
    s)
  (fn stack_push [s:i32 v:i32] -> void
    (let n:i32 (mem.load32 s))
    (mem.store32 (+ s (+ 4 (* n 4))) v)
    (mem.store32 s (+ n 1)))
  (fn stack_pop [s:i32] -> i32
    (req (gt (mem.load32 s) 0))
    (let n:i32 (- (mem.load32 s) 1))
    (mem.store32 s n)
    (mem.load32 (+ s (+ 4 (* n 4)))))
  (fn main [] -> i32
    (let s:i32 (call stack_new 4))
    (call stack_push s 10)
    (call stack_push s 32)
    (+ (call stack_pop s) (call stack_pop s))))     ;; => Int(42)
```

### 12.3 Printing and strings, VM only

```lisp
(module hello
  (fn main [] -> i32
    (sys.print (+ "hello, " "aipl"))
    0))
```
`aipl eval hello.aipl` prints `hello, aipl` then `[AIPL Result]: Int(0)`. `aipl compile hello.aipl` fails: `sys.print not supported in wasm backend: comes with WASI in P6`.

### 12.4 Real threads and atomics, VM only

Function names for `thread.spawn` are written into memory byte by byte; there are no first-class functions yet (P10).

```lisp
(module counter_demo
  (fn worker [arg:i32] -> i32
    (loop i 1 1000 1
      (atomic.add 512 1))       ;; shared counter lives at address 512
    0)

  (fn main [] -> i32
    (mem.store32 512 0)
    ;; write "worker" at address 600
    (mem.store8 600 119) (mem.store8 601 111) (mem.store8 602 114)
    (mem.store8 603 107) (mem.store8 604 101) (mem.store8 605 114)
    (let t1:i32 (thread.spawn 600 6 0))
    (let t2:i32 (thread.spawn 600 6 0))
    (let _a:i32 (thread.join t1))
    (let _b:i32 (thread.join t2))
    (mem.load32 512)))            ;; => Int(2000), deterministically
```
`atomic.add` returns the previous value; the call above is in statement position so the value is dropped. Threads share linear memory and the function table but not locals.

### 12.5 File round-trip, VM only

Paths are `(ptr, len)` pairs into linear memory, matching the WASI convention. `fs.open` flags: `0` read-only, non-zero write/create/truncate. All `fs.*` return `-1` on failure rather than raising.

```lisp
(module file_demo
  (fn main [] -> i32
    ;; path "t.bin" at 700
    (mem.store8 700 116) (mem.store8 701 46) (mem.store8 702 98)
    (mem.store8 703 105) (mem.store8 704 110)
    (mem.store32 800 3735928559)              ;; 0xDEADBEEF, wraps to -559038737
    (let w:i32 (fs.open 700 5 1))
    (let _n:i32 (fs.write w 800 4))
    (let _c:i32 (fs.close w))
    (let r:i32 (fs.open 700 5 0))
    (let _m:i32 (fs.read r 900 4))
    (let _d:i32 (fs.close r))
    (let _x:i32 (fs.delete 700 5))
    (if (eq (mem.load32 900) (mem.load32 800)) 1 0)))   ;; => Int(1)
```

---

## 13. Pitfalls for Code Generators

Each of these is a real failure mode observed when LLMs write AIPL. The fix is in the second column.

| Mistake | Correct form |
|---|---|
| `(f x)` to call a user function | `(call f x)`; bare `(name ...)` is only for built-in ops |
| `(if c (set! x 1))` with no else | `if` always takes three arguments; use `(if c (set! x 1) x)` or `(if c (set! x 1) 0)` |
| `(let x 5)` | `(let x:i32 5)`; the type annotation is mandatory |
| `(let x:i32 5)` twice for the same name, expecting a new scope | AIPL has one flat scope per function today; use a new name or `set!` |
| `(set! y 1)` without a prior `let y` | declare first; there are no implicit globals |
| `(+ n 1.0)` or `(eq n 0.0)` on an `i32` | all operands to one op share one type; write `1` or convert explicitly |
| returning `void` from an `-> i32` function (body ends in `while`/`loop`/`set!` to a `void`) | end the body with a value expression, e.g. the accumulator name |
| using `(and a b)` for short-circuiting | both operands are always evaluated; guard with a nested `if` if the second has side effects |
| `(ptr i32)`, `(fn (i32) -> i32)` in a type position | not parseable yet; pointers are plain `i32` addresses |
| `(let x:i64 5)` or `(+ n 1i64)` where `n` is `i32` | no implicit widening: write `5i64`, or convert with `(i64.extend_s n)`; narrow back with `(i32.wrap x)` |
| `(loop i 0 n 1 ...)` expecting `n` iterations | `loop` is inclusive: this runs `n + 1` times; use `(- n 1)` |
| `(% a b)` with negative `a` expecting a positive result | `%` is `rem_s`; add `b` and take `%` again for a modulo |
| `sys.print`, `fs.*`, `thread.*`, `atomic.*`, `str` in code meant for `aipl compile` | VM-only until WASI (P6) and shared memory land |
| string comparisons `(eq "a" "b")` | works in the VM, but in wasm every `str` is `0`, so it is always `true`; compare bytes in memory for portable code |
| `return`, `break`, `continue`, `else if`, `cond` | do not exist (P11); restructure with `while` + a flag, or nested `if` |
| an extra `)` after the closing `(module` paren | reported as `L:C: unexpected tokens after module end — check for an extra ')'` |
| `inf`, `nan`, `1e9` as literals | not literals; `1e9` is a symbol and will be reported as an undefined variable |
| `(req n > 0)` | contracts are S-expressions: `(req (gt n 0))` |
| `(ens (gt result 0))` | the result is named `res`, always |
| declaring a test "done" because it returned a number | assert the value; see section 10 |
