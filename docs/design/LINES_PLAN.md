# Source lines in compiled crashes

A compiled program that traps prints its call chain (`  at f`). This plan
adds where in the source each frame was: `  at f (shapes.aipl:12:5)`, under
`aipl-run` and in native executables, with both compilers still emitting
identical bytes.

## The table

Each compiler records, for every function it compiles from source, a list
of entries `(offset, file, line, col)`: from that offset in the function's
body (counted from the start of the body, its locals declaration included,
as `wasm_encoder::Function::byte_len` counts), the code belongs to the
expression written at `file:line:col`.

**Which instructions are recorded.** Only the ones a frame can point at: an
instruction that can trap, or a call. By opcode byte: `unreachable` (0x00),
`call` and `call_indirect` (0x10, 0x11), loads and stores (0x28-0x3E),
integer division and remainder (0x6D-0x70, 0x7F-0x82), float-to-integer
truncation (0xA8-0xB1), and every 0xFC- and 0xFE-prefixed instruction
(bulk memory, atomics). Right before emitting one, the compiler records the
position of the innermost expression being compiled, unless the previous
entry already has that position. Recording only at these instructions makes
the table small, and makes it depend only on which expression a trapping
instruction or call belongs to, not on how each compiler shapes its tree.

**Positions.** The innermost expression is tracked with a stack: compiling
an expression pushes its position, finishing it pops. A function's own
position is at the bottom. A `req` or `ens` check (condition and failure
code) is attributed to the contract's position. Compiler-made functions
(`_start`, `aipl.*`, `wasi_thread_start`) have no entries.

**File names.** The entry file by its file name (`shapes.aipl`), an
imported module by its import path (`vec.aipl`, `native/lower.aipl`):
short, the same in both toolchains, and independent of where the files are
on disk. A generic instance's code is the template's file, except a
substituted type argument, which is the file that wrote it.

**Encoding.** A custom section `aipl.lines`, after the name section. All
numbers are unsigned LEB128 except where noted.

```
file count, then each file name (length, UTF-8 bytes), in first-use order
function count, then for each function with entries:
  function index, entry count, then each entry:
    offset delta (from the previous entry's offset, 0 at first)
    file index
    line delta (signed LEB128, from the previous entry's line, 0 at first)
    column
```

## Steps

- **LN1 Rust.** `src/compiler/wasm.rs`: function bodies are written through
  a wrapper that records entries before trapping instructions and calls;
  `compile_expr` pushes and pops positions; the resolver gives each item's
  file its display name; the section is emitted. `aipl-run` reads it and
  prints positions in the call chain.
- **LN2 AIPL code generator.** `aipl_src/codegen.aipl` records the same
  entries (every opcode goes through one function that knows the set
  above). `compile_named(src, len, name)` compiles one file's text, its
  positions in that text; `consts.aipl`'s reprinting keeps an origins map
  back to it. Byte parity in `tests/test_selfhost.rs`.
- **LN3 Multi-file.** `driver.aipl` passes the resolver's and the generics
  pass's origins to the code generator, so `aiplc` names the right files
  and lines; byte parity with the Rust toolchain on multi-module programs
  (`tests/test_resolver_aipl.rs`, the self-compile fixpoint).
- **LN4 Native.** `wasm_reader.aipl` reads the section; `lower.aipl` maps
  entries to machine-code addresses; the backtrace routine prints them, so
  native executables print exactly what `aipl-run` prints.
- **LN5 Docs and checks.** The spec, the fuzzers (the program fuzzer
  already compares `aipl-run` and native output byte for byte), mutation
  checks, and the size and speed cost.
