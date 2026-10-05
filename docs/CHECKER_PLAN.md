# The type checker in AIPL

The self-hosted toolchain (`aipl_src/driver.aipl`: resolver, generics,
constants, code generator) compiles any checked program to the Rust
toolchain's bytes, but it trusts its input: the parser (`src/parser.rs`)
and the checker (`src/checker.rs`) that reject bad programs exist only in
Rust. This plan adds both in AIPL, so `aiplc` rejects what `aipl verify`
rejects, with the same messages, and the Rust front end can retire
(PROGRESS.md "Next steps" 9-10).

## Design

- **A typed syntax tree, built from unions.** `aipl_src/ast.aipl` declares
  the program as AIPL data: `Type`, `Expr` (one variant per form), `FnDef`,
  `StructDef`, `EnumDef`, `UnionDef`, `Module`, mirroring `src/ast.rs`.
  `aipl_src/parser.aipl` builds it from `compiler.aipl`'s generic tree and
  reports what `src/parser.rs` reports. `aipl_src/printer.aipl` prints it
  back as `src/printer.rs` does. This is the first large program written on
  unions and `match`; the code generator can move onto the same tree later.
- **`aipl_src/checker.aipl` mirrors `src/checker.rs` rule for rule**, with
  the same message text, including the Rust spelling of types in messages
  (`Ptr(Struct("Point"))`). Changing that spelling to AIPL syntax (audit
  D2) is a later change made to both at once.
- **Positions.** Messages start with `line:col` (columns count characters,
  as the Rust tokenizer does). Tokens and nodes gain the source offset they
  start at; line and column are computed from it when an error is reported.
- **Input.** The checker runs on the flat program the AIPL resolver and
  generics pass produce, with constants and enum members expanded as
  `src/consts.rs` does (members become `(enum.cast E n)`, enum types stay),
  and before `consts.aipl` erases enums for the code generator.

## The safety net

Each step keeps the existing tests green and adds a parity check against
the Rust front end, always on the same text (the AIPL resolver's flat
output, which both sides can read), so positions agree:

1. **Print parity:** for every repository program, AIPL parse then print
   equals Rust parse then print, byte for byte.
2. **Acceptance parity:** every repository program is accepted by both.
3. **Message parity:** a corpus of rejected programs (every rule table in
   `tests/`, the parser's error cases, and the operator sweep that found
   the audit-fix bugs) gets the identical message from both, position
   included. A file-path prefix that Rust adds to parse errors is ignored.
4. **Mutation checks** on each new test.

Large inputs run in the compiled toolchain under wasmtime (as
`tests/test_resolver_aipl.rs` does), not in the VM.

## Steps

| Step | What | Status |
|---|---|---|
| CK1 | Positions: tokens and nodes carry their source offset; a `line:col` helper counting characters. The AIPL tokenizer's literal rules brought in line with Rust's where they differ (out-of-range integers, string escapes, unterminated strings), each with an error | Done 2026-10-05. Both tokenizers checked token by token (kind, line:col, value) on every repository file and 17 tricky inputs (`tests/test_checker_aipl.rs`, mutation-checked). Rust fixes found on the way: a form feed or no-break space hung the tokenizer forever (now an error naming the character), and `i32` literals beyond 32 bits wrapped silently (now an error; `-2147483648`..`4294967295` allowed). The AIPL passes now print integers as written, and codegen.aipl reads `+` signs and reports exponents as 973 |
| CK2 | `ast.aipl` and `parser.aipl` for module items and types (structs, enums, unions, functions, imports-free modules), `printer.aipl`; print parity on items | Done 2026-10-05, with CK3. `ast.aipl` is the tree as unions (`Type`, `Expr`, `Contract`), the first large AIPL program written on them; it needed a wildcard binder `_` (added to the language) |
| CK3 | Expressions in the parser, every form; print parity on every repository program | Done 2026-10-05. `parser.aipl` ports `src/parser.rs`'s recursive descent over the token array (its errors depend on the exact token), with the first error recorded and the parse ended. Two tests (`tests/test_checker_aipl.rs`, the front end compiled to wasm under wasmtime): every repository program as the Rust printer prints it round-trips through parser.aipl and printer.aipl byte for byte; and the original sources, plus a corpus using every form and operator, print the same through both parsers. Mutation-checked; the second test exists because `cond` and `gteu` never appear in printed repository programs |
| CK4 | Parse errors: every `src/parser.rs` message; message parity on its cases | |
| CK5 | Constants: a typed expansion mode in `consts.aipl` matching `src/consts.rs` (enum members to `(enum.cast E n)`), with its errors | |
| CK6 | Checker, module level: registering structs, enums, unions, functions; `validate_type`; type layout | |
| CK7 | Checker, expressions: control flow, calls, scoping, `let`/`set!`, contracts, `return`/`break`/`continue` | |
| CK8 | Checker, operators (the full table, with the address rules) | |
| CK9 | Checker, data: structs, arrays, pointers, references, results, enums, unions and `match` | |
| CK10 | Message parity on the whole rejection corpus; acceptance parity on every repository program | |
| CK11 | Wiring: `driver.aipl` checks before compiling (an `aiplc` exit status and message for a rejected program), `aipl verify --self`; docs (AIPL_SPEC.md 6.4, README, PROGRESS) | |

Each step is one or more commits on `features/checker-aipl`, merged into
`development` when the safety net is green.
