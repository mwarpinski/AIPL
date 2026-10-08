# AIPL

[![CI](https://github.com/mwarpinski/AIPL/actions/workflows/ci.yml/badge.svg)](https://github.com/mwarpinski/AIPL/actions/workflows/ci.yml)

AIPL is a small, statically typed systems language meant to be written by
AI coding agents. It compiles to WebAssembly and to native Linux x86-64
executables, and its compiler can compile itself.

## Why

When a model writes code, the mistakes are predictable: an operator
precedence it misread, an implicit conversion, an array index one past the
end, a bug that crashes far from its cause. AIPL is designed so that fewer
of those mistakes get written, and those that are caught point straight at
the problem:

- **One way to write each thing.** Prefix S-expressions with no operator
  precedence and no indentation rules. Every parameter, return value, local,
  and field has its type written out. Nothing converts implicitly.
- **Checks on the parts that go wrong.** Contracts (`req`, `ens`) run on
  every call, array indexes are bounds-checked, and pointers are typed, in
  compiled code as well as in the interpreter.
- **Errors an agent can act on.** Every compile error names the file, line,
  and column, and every failing function reports its own. A crash in a
  compiled program prints its call chain with a position for each frame.

Whether this actually helps agents write correct programs more often has
not been measured yet. An experiment comparing agents writing the same
tasks in AIPL and in other languages is the next major step
([ROADMAP.md](ROADMAP.md)).

## An example

```lisp
(module gcd
  (import io)

  ;; Greatest common divisor. The contracts are checked on every call.
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

  (fn main [] -> i32
    (call io.println_int "gcd: " (call gcd 1071 462))
    (call io.println_int "gcd: " (call gcd 10 0))
    0))
```

Compiled to a native executable and run, the second call breaks the
contract:

```
$ aipl compile --exe gcd.aipl -o gcd && ./gcd
gcd: 21
./gcd: Pre-condition failed in 'gcd': (req (and (gt a 0) (gt b 0))) with a = 10, b = 0
  at gcd (gcd.aipl:6:10)
  at main (gcd.aipl:18:34)
```

A type mistake is caught before the program runs:

```
$ aipl verify bad.aipl
Error: bad.aipl: 18:34: Arg 1 of 'gcd' expects i32, got bool
```

## Install

AIPL is built from source with a stable Rust toolchain:

```bash
git clone https://github.com/mwarpinski/AIPL.git && cd AIPL
cargo build --release
```

This builds `target/release/aipl` (the compiler, interpreter, and checker)
and `target/release/aipl-run` (the launcher for compiled WebAssembly). The
binaries find the standard library in the clone they were built from, so
keep it in place. Native executables need Linux x86-64; the WebAssembly path
should work wherever wasmtime runs, but only Linux is tested.

## Try it

With `target/release` on your `PATH`:

```bash
aipl verify examples/word_count.aipl            # parse and type-check
aipl eval examples/math_core.aipl               # run main in the interpreter
aipl compile examples/word_count.aipl -o wc.wasm
aipl run wc.wasm -- README.md                   # run compiled WebAssembly
aipl compile --exe examples/word_count.aipl -o wc
./wc README.md                                  # a native executable
```

The self-hosted compiler, built as a native executable, compiles itself in
about half a second to the same bytes the Rust compiler produces:

```bash
aipl compile --exe aipl_src/driver.aipl -o aiplc
./aiplc aipl_src/driver.aipl aiplc.wasm
```

## What works today

- **The language:** `i32`, `i64`, `f32`, `f64`, `bool`, and `str`; structs
  behind typed pointers; arrays; enums; unions with an exhaustive `match`;
  results; function references; generic structs and functions; constants;
  modules and imports; threads and atomics.
- **Three ways to run a program,** which must agree: an interpreter for
  quick runs, WebAssembly with WASI (files, stdio, the command line, the
  environment, clocks, threads), and native Linux x86-64 executables with no
  runtime (`word_count` is 77 KB).
- **A standard library** with printing and file I/O, strings, number
  formatting and exact float parsing, growable vectors, hash maps, a string
  builder, big integers, timing, and allocators. Memory you free comes from
  an allocator you pass explicitly, as in Zig: a general-purpose heap that
  stops the program on a double free or a write after free, an arena, or one
  you write.
- **A self-hosted toolchain.** The resolver, type checker, code generator,
  and native backend are written in AIPL. They produce the same bytes and
  the same error messages as the Rust implementation, and the compiler
  rebuilds itself. Building it the first time still takes the Rust
  toolchain.

AIPL is **not** memory-safe: raw memory operations (`mem.load32` on a
computed address) and casts are unchecked, and so are reads of freed heap
memory. What it does check is listed above.

It is also not fast yet. On eight benchmarks, native code takes 1.7 to 40
times as long as C compiled with `gcc -O2`, and WebAssembly under wasmtime
0.8 to 14 times as long. The native backend is a simple one-pass translator.
[docs/BENCHMARKS.md](docs/BENCHMARKS.md) has every number and why.

## What is next

In order ([ROADMAP.md](ROADMAP.md) has the details and everything else that
is missing):

1. **Launch readiness** ([docs/LAUNCH_CHECKLIST.md](docs/LAUNCH_CHECKLIST.md)):
   positions in the interpreter's runtime errors, and the experiment
   measuring how well agents write AIPL.
2. **The language:** option types and generic unions, constraints on
   generics, visibility (`pub`), small structs by value.
3. **The standard library:** JSON, directories, running processes, then
   networking.
4. **Retiring the Rust compiler:** rebuilding the toolchain from a
   checked-in compiled seed, so Rust is needed only for the launcher.
5. **Faster native code:** a translator that keeps values in registers.
6. **Packages:** a registry with versioned packages, dependency resolution,
   and lock files.

## How it is made and tested

AIPL is designed by Matt Warpinski and implemented with AI coding agents.
Nothing is taken on trust: every change has to pass checks that compare
independent implementations against each other.

- **294 Rust tests** and an AIPL test suite, run on every push by CI.
- **The interpreter, WebAssembly, and native code must agree.** Differential
  tests run the same programs every way, and a fuzzer generates random
  well-typed programs and compares their output and failures (CI runs 500
  per push). It has found and fixed real bugs.
- **Two compilers, byte for byte.** The Rust and the AIPL toolchains must
  produce identical output; a second fuzzer mutates programs and compares
  the two (3,000 per push).
- **Tests that are themselves checked.** New tests are checked by planting
  bugs in the code under test and confirming the tests catch them.

## Documentation

- [AIPL_SPEC.md](AIPL_SPEC.md): the language as implemented, with its
  pitfalls for code generators.
- [PROMPT_GUIDE_FOR_AIS.md](PROMPT_GUIDE_FOR_AIS.md): a compact guide to
  give an agent that will write AIPL.
- [ROADMAP.md](ROADMAP.md): what comes next, and what AIPL does not do yet.
- [DEVELOPING.md](DEVELOPING.md): building, testing, the repository layout,
  and the conventions the work follows.
- [docs/](docs/): benchmarks, the launch checklist, design records of the
  larger pieces of work, and history.

## License

MIT ([LICENSE](LICENSE)).
