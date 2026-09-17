# AIPL (AI Programming Language)

> **Machine-Native, Token-Dense, Formally Verifiable Dual-Target Systems Language, Self-Hosting Compiler & Autonomous Agent Ecosystem**

---

## 1. Executive Summary & Core Philosophy

**AIPL (AI Programming Language)** is a machine-native, token-dense, formally verifiable programming language, self-hosting compiler pipeline, and Intermediate Representation (IR) designed exclusively for AI agent consumption, high-performance WebAssembly compilation, linear memory manipulation, atomic swarm concurrency, and bare-metal native execution.

Traditional programming languages designed for human readability introduce severe cognitive friction and token waste for Large Language Models (LLMs) and AI agents:
- Indentation semantics, semicolon rules, and complex operator precedence create syntactic ambiguity.
- Opaque human stack traces waste thousands of LLM context window tokens.
- Relying on heavy external toolchains (Cargo, npm, pip) breaks sovereign execution loops.

AIPL solves this by introducing a dual-representation architecture with formal mathematical guardrails, structured machine diagnostics, and zero external language dependencies.

---

## 2. Core Architecture & Language Features

### Dual Syntax Representation
1. **Canonical S-Expression Format (`.aipl`)**: Parenthesis-delimited, context-free AST eliminating human syntactic ambiguities.
2. **Compact Binary AST Payload (`.baipl`)**: 1-byte opcode encoded MessagePack binary payload for zero-parse inter-agent serialization over IPC, HTTP, or gRPC.

### Formal Verification Contracts
Functions enforce mathematical boundaries evaluated statically before compilation:
```lisp
(fn db_read_slot [ptr:i32 offset:i32] -> i32
  (req (gt ptr 0))
  (req (gte offset 0))
  (ens (gte res 0))
  (mem.load32 (+ ptr offset)))
```

### 20-Byte Machine-Parsable Diagnostics
Instead of verbose human stack traces, AIPL emits structured 20-byte binary diagnostic payloads (`aipl_src/diagnostics.aipl`) containing:
- **Error Category**: `ERR_CONTRACT_VIOLATION` (`1001`), `ERR_TYPE_MISMATCH` (`1002`), `ERR_MEMORY_BOUNDS` (`1003`), `ERR_SYNTAX_AST` (`1004`).
- **Location Trace**: Function ID, AST node index, and linear memory offset.

### Raw Systems Primitives
- **Linear Memory Loads & Stores**: `mem.load32`, `mem.store32`, `mem.load64`, `mem.store64`, `mem.alloc`, `mem.free`.
- **Bitwise Operations**: `shl`, `shr`, `bitand`, `bitor`, `^`.
- **Atomic Swarm Concurrency**: `atomic.lock`, `atomic.unlock`, `atomic.add`, `atomic.cas`.
- **SIMD AI Vector Embeddings**: `vec.dot` (native dot-product vector similarity scoring for RAG lookups).
- **Result Match Error Handling**: `ok`, `err`, `match_result`.

---

## 3. Dual-Target Compilation Pipeline

AIPL features a dual-target compilation architecture built natively in S-expressions:

```
                          ┌───────────────────────────┐
                          │   AIPL Source (.aipl)     │
                          └─────────────┬─────────────┘
                                        │
                                        ▼
                          ┌───────────────────────────┐
                          │  Tokenizer & Dynamic AST  │
                          │   Symbol Table Resolver   │
                          └─────────────┬─────────────┘
                                        │
                         ┌──────────────┴──────────────┐
                         │ compile_to_target Router   │
                         └──────┬──────────────┬───────┘
                                │              │
            Target 0: Wasm      │              │      Target 1: Native ELF64
                                ▼              ▼
                    ┌─────────────────┐  ┌───────────────────┐
                    │ Wasm Emitter    │  │ Heavy Optimizer   │
                    │ & LEB128        │  │ Constant Folding  │
                    └────────┬────────┘  │ RegAlloc (x86_64) │
                             │           └─────────┬─────────┘
                             ▼                     │
                    ┌─────────────────┐            ▼
                    │ Standalone      │  ┌───────────────────┐
                    │ .wasm Binary    │  │ Bare-Metal Linux  │
                    └─────────────────┘  │ ELF64 Executable  │
                                         └───────────────────┘
```

1. **Portability Target (`--target wasm`)**: Compiles source modules into standard, spec-compliant WebAssembly binaries (`.wasm`) via `wasm_emitter.aipl`.
2. **Bare-Metal Speed Target (`--target native`)**: Routes AST nodes through `optimizer.aipl` (constant tree folding & x86_64 physical register allocation) and `elf_emitter.aipl` to generate raw 64-bit Linux ELF executables executing at uncompromised assembly speed.

---

## 4. Sovereign AIPL Ecosystem Toolchain (`aipl_src/`)

The entire compiler toolchain is written 100% in canonical AIPL S-expressions:

- **[aipl_src/diagnostics.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/diagnostics.aipl)**: Structured 20-byte diagnostic payload engine and static type guardrails.
- **[aipl_src/aipl_test.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl_test.aipl)**: Native sovereign test runner, content-level byte asserter, and negative contract trap verifier.
- **[aipl_src/wasm_emitter.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/wasm_emitter.aipl)**: LEB128 varint encoder, magic header, and section builders (Type, Function, Memory, Export, Code, Data).
- **[aipl_src/optimizer.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/optimizer.aipl)**: Heavy optimizer pass performing static constant folding and physical x86_64 register mapping (`RAX`, `RBX`, `RCX`, `RDX`, `RDI`).
- **[aipl_src/elf_emitter.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/elf_emitter.aipl)**: Bare-metal 64-bit Linux ELF header (`\x7fELF`) and x86_64 machine code generator (`mov`, `add`, `sub`, `imul`, `syscall`).
- **[aipl_src/compiler.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/compiler.aipl)**: Dynamic AST parser, symbol table index resolver, and dual-target compiler router.
- **[aipl_src/sovereign_toolchain.aipl](file:///home/matt/Projects/Repos/AIPL/aipl_src/sovereign_toolchain.aipl)**: Unified master module packaging all passes into a single standalone artifact (`aipl_sovereign_toolchain.wasm`).

---

## 5. Applications & Infrastructure Built in AIPL

- **[AISQL Hybrid Database Engine](file:///home/matt/Projects/Repos/AIPL/examples/aipl_database/aisql_engine.aipl)**: Real-memory hybrid relational + vector embedding database engine written 100% in AIPL S-expressions, featuring schema definition, hash index lookups, and AI vector similarity dot-product scoring.
- **Native System Binaries**: `bin/aisql` and `./bin/aipl-test` CLI runners.

---

## 6. Future State: Total Severance of Rust & Host Code

The project follows a 3-stage bootstrap model to achieve total self-reliance:

```
┌───────────────────────────┐     ┌───────────────────────────┐     ┌───────────────────────────┐
│ Stage 0: Rust Bootstrap   │ ──► │ Stage 1: Wasm Self-Host   │ ──► │ Stage 2: Absolute         │
│ Cargo compiler used as    │     │ AIPL compiler compiled to │     │ Sovereign Native ELF      │
│ temporary sandbox driver. │     │ .wasm runs inside Wasm.   │     │ Sever all Rust/JS/host    │
└───────────────────────────┘     └───────────────────────────┘     │ dependencies. Runs        │
                                                                    │ directly on Linux kernel. │
                                                                    └───────────────────────────┘
```

1. **Stage 0 (Current Bootstrap)**: Rust driver (`cargo run --bin aipl`) serves as the initial sandbox verifier and Stage 0 bootstrap compiler.
2. **Stage 1 (Self-Hosting Wasm)**: `aipl_compiler.wasm` / `aipl_sovereign_toolchain.wasm` running inside a lightweight Wasm runtime compiles AIPL files without calling Rust or Cargo.
3. **Stage 2 (Absolute Sovereignty)**: Severing 100% of host language code (Rust, JS, C, Python). The self-hosted AIPL compiler compiled directly to a native Linux ELF64 binary executes on bare-metal hardware with zero virtual machine overhead.

---

## 7. Command Reference

### Verify AIPL Source Code
```bash
cargo run --bin aipl -- verify aipl_src/sovereign_toolchain.aipl
```

### Compile AIPL to WebAssembly
```bash
cargo run --bin aipl -- compile aipl_src/compiler.aipl -o aipl_compiler.wasm
```

### Run Native Sovereign AIPL Test Harness
```bash
./bin/aipl-test
```

### Execute AISQL Vector & Relational Database Demo
```bash
./bin/aisql demo
```

### Run Workspace Test Suite
```bash
cargo test
```
