# Rewriting the self-hosted code generator on typed data

**Status: done (2026-10-05).** Kept as the design record.

`aipl_src/codegen.aipl` (about 3000 lines) is the self-hosted wasm code
generator, and everything else trusts it: the AIPL toolchain compiles itself
with it, and the native backend translates its output. It was written before
AIPL had structs, enums, or constants, so all of its data is raw memory
(`mem.load32`/`mem.store32` at computed offsets), and every kind of thing is
a bare number: AST node kinds (6, 7, 20...), keywords (0-86 for ops, 100-115
for forms, 200-209 for types), static types (200-212), and compile errors
(90-101, 768, 971...). docs/history/AUDIT_2026-10.md (S2) explains why that is the
riskiest code in the repository.

This plan moves it, and the tokenizer/parser in `aipl_src/compiler.aipl`, onto
typed structs, enums, and constants in small steps.

## The safety net

The output must not change. Every step keeps these green:

- `tests/test_selfhost.rs`: the self-hosted compiler's output equals the Rust
  backend's, byte for byte, on over 40 programs, including codegen.aipl
  compiling itself.
- `tests/test_resolver_aipl.rs`: the compiled AIPL toolchain compiles every
  repository program, the native backend, and itself to the Rust toolchain's
  bytes (the bootstrap fixpoint).
- `aipl test aipl_src/test_suite.aipl` (codegen's own self-tests) and the rest
  of `cargo test`.

Enums do the rest: once a value has an enum type, the Rust checker rejects
every place that still treats it as a number, so a step is finished when the
file type-checks, not when a search finds nothing.

## Steps

| Step | What | Status |
|---|---|---|
| CR1 | Keywords: `enum Kw` with the existing values; the hand-encoded byte table and its 130 sequential comparisons become a `strmap` from keyword text, filled from string literals. `classify_keyword` returns `Kw`; numeric ranges (`(gte kw 73)`) become named predicates | Done 2026-10-05. The hand-encoded WASI and export names became string literals too (`emit_name`). The keyword lookup is one hash lookup instead of up to 130 comparisons, and test_selfhost (codegen in the VM) runs in 14 s instead of about 20 |
| CR2 | AST nodes: `enum NodeKind` in compiler.aipl; codegen reads `(ptr compiler.Node)` with `get` instead of `node_kind`/`node_a` on raw addresses | Done 2026-10-05. `compiler.Kind` types tokens and nodes in compiler.aipl, resolver.aipl, generics.aipl, consts.aipl, and codegen; every node in codegen is a `(ptr compiler.Node)`, and a group's children are reached with `first_child` rather than its `a` field. The function table still stores node addresses as raw words (`ptr.addr`/`ptr.cast` at that boundary until CR5) |
| CR3 | Static types: `enum Ty` (the 200-212 ids) and its conversions to wasm value types and block types | Done 2026-10-05. Types come from keywords through `ty_of_kw` rather than by reusing the keyword's number; the type predicates (`is_memory_type`, value and block types, load and store opcodes) are written over named types. Tables still store types as raw words (`enum.cast`/`enum.ord` at that boundary until CR5) |
| CR4 | Compile errors: `enum CgErr` with the documented codes (AIPL_SPEC.md 6.4); the codes stay the same, since hosts read them from cell 4 | Done 2026-10-05: all 32 sites name their error (`CgErr.unknown_symbol`), and the enum documents each code |
| CR5 | Context: one `Cg` struct (the source, the current function's locals and count, the module's functions and count) instead of the five parameters every compile function threaded, so `compile_expr [src_ptr node out_ptr locals_base locals_count func_table_base func_count]` became `compile_expr [cg node out_ptr]` | Done 2026-10-05, ahead of the tables (planned as CR6's second half) so signatures change once. 57 functions and 250 calls were rewritten by a script that only dropped arguments equal to the state they stand for |
| CR6 | Tables as typed structs inside `Cg`: functions, locals, structs and fields, string literals, `call_ref` signatures, the label stack, WASI imports; `Cg` replaces runtime cells 8-32. Cells 4, 44, 48, and 60 stay, as the host interface | Done 2026-10-05 (CR6a-g): `FnInfo`, `Local`, `StructInfo`/`Field`, `Sig`, `StrLit` plus a `buf.Buf` blob, and enums `Wasi` and `Label`, all held by `Cg`. Cell 16 keeps the keyword map, which is built once and reused across compiles. Tables are bounded arrays, so counts stop at their limits after reporting the error (a new test checks errors 92 and 93). Raw memory operations fell from 422 to 252, nearly all of them now writing output bytes (CR7) or reading source bytes |
| CR7 | Output: a buffer with a position instead of `out_ptr` arithmetic and returned byte counts; wasm opcodes and section ids as constants | Done 2026-10-05 except the opcode constants (moved to CR9). An `Out` cursor; every emitter and compile function appends to one; the module assembler builds each section in its own cursor (`emit_section`), function types through one `emit_func_type`, and the WASI signatures as a table (`wasi_sig`). Converted function by function with temporary old-style wrappers, all removed. Raw memory operations: 38 (from 422), reading source bytes and the host cells |
| CR8 | compiler.aipl: tokens and nodes built with `new`/`put`, the token record read through a struct | Done 2026-10-05. Character codes are named constants (`CH_LPAREN`), keywords like `true` and `->` are matched against string literals (`span_is`), the parser's state is a `Parser` struct instead of two cells at the front of the node buffer, and the self-tests spell their inputs as string literals instead of byte codes. The dead `resolve_symbol_index` is gone. 535 to 321 lines; raw memory operations 150 to 17 |
| CR9 | Documentation (AIPL_SPEC.md 6.4 and 7.9, ROADMAP.md), the audit, and a before/after comparison (size, speed of compiling the toolchain) | Done 2026-10-05, with the opcode constants from CR7 (`OP_IF`, `OP_I32_ADD`, `BLOCK_EMPTY`, `VT_I32`, ...). Results below |

Each step is one or more commits on `features/codegen-rewrite`, merged into
`development` when the safety net is green.

## Results

| | Before (5e7aede) | After |
|---|---|---|
| codegen.aipl lines | 3077 | 2986 (including 70 lines of named opcodes and comments) |
| raw `mem.load`/`mem.store` in codegen.aipl | 422 | 38 (reading source bytes, the host cells, `put_byte`) |
| typed field accesses (`get`/`put`) in codegen.aipl | 0 | 281 |
| compiler.aipl lines / raw memory operations | 535 / 150 | 321 / 17 |
| the toolchain compiled to wasm (driver.aipl) | 105,058 bytes | 84,293 bytes |
| the native compiler (`aipl compile --exe aipl_src/driver.aipl`) | 386 KB | 311 KB |
| native compiler on driver.aipl / native.aipl / knucleotide.aipl | 0.124 / 0.102 / 0.023 s | 0.090 / 0.074 / 0.016 s (about 1.4x) |
| test_selfhost (codegen run in the VM) | about 20 s | about 12 s |

Output is byte-identical: both compilers produce the same bytes for the same input, and the Rust toolchain's.

How it was done: mostly by scripts driven by the checker's errors (run `aipl verify`, fix what it reports at its position, repeat), with each kind of value moved to an enum or a typed pointer so the checker found every place that still treated it as a number. Byte parity after each batch caught every mistake the scripts made (a bridge rewrite that dropped calls, a misplaced bracket after a comment, a lost opcode before an immediate).
