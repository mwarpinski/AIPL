# AIPL Formal Specification (v2.0 Systems & Concurrency Edition)
## AI Programming Language: Machine-Native Formal Specification

AIPL (AI Programming Language) is an ultra-dense, non-ambiguous, formally verifiable systems programming language, self-hosting compiler, and Intermediate Representation (IR) designed exclusively for AI agent consumption, high-performance WebAssembly compilation, linear memory manipulation, atomic swarm concurrency, and native vector embedding RAG database operations.

---

## 1. Syntax Architecture: Dual Representation

AIPL has two canonical forms:
1. **S-Expression Canonical Text Representation (`.aipl`)**: Context-free, parenthesis-delimited AST. Eliminates human syntactic ambiguities (no operator precedence rules, no indentation semantics, no semicolon requirements).
2. **Compact Binary AST Payload (`.baipl`)**: 1-byte opcode encoded MessagePack binary representation used for instant zero-parse serialization between AI swarms over HTTP, gRPC, or IPC.

---

## 2. Updated Formal Grammar (EBNF)

```ebnf
program        ::= "(" "module" identifier statement* ")" ;
statement      ::= fn_def | let_stmt | set_stmt | expr ;

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
                 | "(" "call" expr expr* ")"
                 | "(" "ok" expr ")"
                 | "(" "err" expr ")"
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" op expr expr* ")" ;

op             ::= arithmetic_op | bitwise_op | memory_op | atomic_op | comp_op | vector_op ;

arithmetic_op  ::= "+" | "-" | "*" | "/" | "%" ;
bitwise_op     ::= "^" | "shl" | "shr" | "bitand" | "bitor" ;
memory_op      ::= "mem.load8" | "mem.load32" | "mem.load64" | "mem.load_f32" | "mem.load_f64"
                 | "mem.store8" | "mem.store32" | "mem.store64" | "mem.store_f32" | "mem.store_f64"
                 | "mem.alloc" | "mem.free" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "and" | "or" | "not" ;
vector_op      ::= "vec.dot" | "matmul" | "arr.get" | "arr.set" | "dom.elem" | "dom.mount" ;
```

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

## 4. Systems Operations (Memory, Atomics & Vector Search)

### A. Raw WebAssembly Linear Memory Loads & Stores
- `(mem.load32 ptr)` -> Reads 4 bytes from linear memory offset `ptr` (`i32.load`).
- `(mem.store32 ptr val)` -> Writes 4 bytes to linear memory offset `ptr` (`i32.store`).
- `(mem.alloc size)` -> Dynamic heap allocation tracking.

### B. Atomic Swarm Synchronization
- `(atomic.lock mutex_ptr)` -> Acquires thread-safe mutex lock.
- `(atomic.unlock mutex_ptr)` -> Releases mutex lock.
- `(atomic.add ptr val)` -> Atomic memory addition.

### C. Hybrid Relational + AI Vector Similarity Search
- `(vec.dot v1 v2)` -> SIMD vector dot product similarity scoring for AI RAG embeddings.

---

## 5. WebAssembly Binary Execution Semantics

AIPL code maps 1-to-1 to WebAssembly binary opcodes:
- `(mem.load32 ptr)` -> `i32.load (MemArg { offset: 0, align: 2 })`
- `(mem.store32 ptr val)` -> `i32.store (MemArg { offset: 0, align: 2 })`
- `(shl a b)` -> `i32.shl`
- `(shr a b)` -> `i32.shr_s`
- `(bitand a b)` -> `i32.and`
- `(bitor a b)` -> `i32.or`
