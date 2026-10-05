# Rewriting the self-hosted code generator on typed data

`aipl_src/codegen.aipl` (about 3000 lines) is the self-hosted wasm code
generator, and everything else trusts it: the AIPL toolchain compiles itself
with it, and the native backend translates its output. It was written before
AIPL had structs, enums, or constants, so all of its data is raw memory
(`mem.load32`/`mem.store32` at computed offsets), and every kind of thing is
a bare number: AST node kinds (6, 7, 20...), keywords (0-86 for ops, 100-115
for forms, 200-209 for types), static types (200-212), and compile errors
(90-101, 768, 971...). AIPL_Structural_Audit.md (S2) explains why that is the
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
| CR2 | AST nodes: `enum NodeKind` in compiler.aipl; codegen reads `(ptr compiler.Node)` with `get` instead of `node_kind`/`node_a` on raw addresses | |
| CR3 | Static types: `enum Ty` (the 200-212 ids) and its conversions to wasm value types and block types | |
| CR4 | Compile errors: `enum CgErr` with the documented codes (AIPL_SPEC.md 6.4); the codes stay the same, since hosts read them from cell 4 | |
| CR5 | Tables as structs: functions, locals, structs and fields, string literals, `call_ref` signatures, the label stack, WASI imports; one state struct replaces runtime cells 8-32. Cells 4, 44, 48, and 60 stay, as the host interface | |
| CR6 | Output and context: an output buffer with a position instead of `out_ptr` arithmetic and returned byte counts; one context struct instead of the seven parameters every compile function threads; wasm opcodes and section ids as constants | |
| CR7 | compiler.aipl: tokens and nodes built with `new`/`put`, the token record read through a struct | |
| CR8 | Documentation (AIPL_SPEC.md 6.4 and 7.9, LANGUAGE_GAPS.md 6), the audit, and a before/after comparison (size, speed of compiling the toolchain) | |

Each step is one or more commits on `features/codegen-rewrite`, merged into
`development` when the safety net is green.
