# AIPL

[![CI](https://github.com/mwarpinski/AIPL/actions/workflows/ci.yml/badge.svg)](https://github.com/mwarpinski/AIPL/actions/workflows/ci.yml)

AIPL is a small, statically typed systems language meant to be written by
AI coding agents. It compiles to WebAssembly and to native Linux x86-64
executables, and its compiler can compile itself.

**Status: early, and honest about it.** The language, the standard library,
and both compilers work and are heavily tested ([what works](#what-works-today),
[how it is tested](#how-it-is-made-and-tested)). Native executables are Linux
x86-64 only; WebAssembly runs wherever wasmtime does, but only Linux is tested.
There are no packages yet, and the question the project exists to answer,
whether agents write correct programs more often in AIPL than in other
languages, has not been measured yet ([the plan](#what-is-next)).

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

It is verbose on purpose, and it is not for people who want to write
less; [docs/FAQ.md](docs/FAQ.md) answers the first questions people ask
(why S-expressions, why not Rust, Zig, or Python).

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
    (println "gcd: {(call gcd 1071 462)}")
    (println "gcd: {(call gcd 10 0)}")
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

A type mistake, such as `(call gcd 10 true)` on line 18, is caught before
the program runs (arguments are counted from 0):

```
$ aipl verify bad.aipl
Error: bad.aipl: 18:34: Arg 1 of 'gcd' expects i32, got bool
```

## Install

The installer needs git, a stable Rust toolchain
([rustup.rs](https://rustup.rs)), and a C compiler for linking (e.g.
`build-essential`). It builds AIPL from source into `~/.aipl` and puts
`aipl` and `aipl-run` on your `PATH`; nothing runs as root:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh | sh
```

To read it before running it (it is about 200 lines of POSIX `sh`):

```bash
curl --proto '=https' --tlsv1.2 -sSfO https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh
less install.sh && sh install.sh
```

`sh install.sh --help` lists the options (`--ref` to install a branch, tag,
or commit; `--no-modify-path`); `sh install.sh --uninstall` removes it. Run
the installer again to update.

Or build a clone by hand:

```bash
git clone https://github.com/mwarpinski/AIPL.git && cd AIPL
cargo build --release
```

This builds `target/release/aipl` (the compiler, interpreter, and checker)
and `target/release/aipl-run` (the launcher for compiled WebAssembly). Either
way, `aipl` carries the standard library inside it, so a copied or installed
one needs nothing beside it (run from a clone's `target/`, it reads that
clone's `aipl_src/`, so an edit there shows at once).

## Your first program

With `aipl` on your `PATH`, save this as `hello.aipl`:

```
(module hello
  (fn main [] -> i32
    (println "hello, aipl")
    0))
```

and run it three ways (the results must agree):

```bash
aipl eval hello.aipl                    # in the interpreter
aipl compile hello.aipl -o hello.wasm && aipl run hello.wasm     # WebAssembly
aipl compile --exe hello.aipl -o hello && ./hello                # a native executable, 64 KB
```

`main` returns the exit status. [docs/TOUR.md](docs/TOUR.md) walks through
the rest of the language in ten minutes, with programs you can run.

## Try the tools

From the source directory (`~/.aipl/src` after the installer):

```bash
aipl verify examples/word_count.aipl            # parse and type-check
aipl doc text                                   # a module's functions and types, with their comments
aipl doc --find split                           # search the standard library
aipl test rows.aipl                             # run a file's (test ...) items, each alone
aipl watch scratch.aipl                         # run it again on every save
aipl eval -e '(call str.find (call str.from_str "a,b") (call str.from_str ","))'   # prints 1 : i32
aipl compile examples/word_count.aipl -o wc.wasm
aipl run wc.wasm -- README.md                   # run compiled WebAssembly
aipl compile --exe examples/word_count.aipl -o wc
./wc README.md                                  # a native executable
```

To have an agent write AIPL, give it [PROMPT_GUIDE_FOR_AIS.md](PROMPT_GUIDE_FOR_AIS.md)
(a compact guide) and let it use `aipl doc` for the library.

## A real program

`examples/jsonpp.aipl` is a JSON pretty-printer: 211 lines, 56 of them its
own tests, with the standard library doing the parsing, the options, and
the output. An AI agent wrote it, with the guide, `aipl doc`, and the other
examples to hand; that is not a cold run (the A/B harness in
`tools/harness/` measures that). It checks out against Python's
`json.dumps` on generated documents (`tests/test_showcase.rs`).

```bash
aipl test examples/jsonpp.aipl                        # its tests
aipl compile --exe examples/jsonpp.aipl -o jsonpp     # 0.3 s, a 319 KB executable
echo '{"b":[1,2,{}],"a":null}' | ./jsonpp --sort
```

```
{
  "a": null,
  "b": [
    1,
    2,
    {}
  ]
}
```

`--indent N`, `--compact`, and `--query PATH` (as in `a.b[2].c`) are there
too; a document that is not JSON is reported as `FILE:LINE:COLUMN: what`.

## Build AIPL with AIPL

The compiler is written in AIPL, and on Linux x86-64 it compiles itself to a
native executable in about 3 seconds. Without Rust, start from the
checked-in seed. From the source directory, this script builds the compiler
with the seed, builds it again with the result, and checks that a third build
is identical (about 8 seconds):

```bash
sh bootstrap/build.sh               # the result is target/bootstrap/aipl
```

The AIPL-built compiler does `compile` only (to `.wasm`, or with `--exe` to an
executable), and it compiles every program to the same bytes as the Rust
one, which stays as the referee: `cargo test` checks the seed's build byte for
byte against it. [bootstrap/README.md](bootstrap/README.md) has the other
ways to start, where the compiler finds the library, and why.

## What works today

- **The language:** `i32`, `i64`, `f32`, `f64`, `bool`, and `str`, and
  8- and 16-bit fields and array elements; structs behind typed pointers,
  and small immutable structs held by value; arrays; enums; unions with an
  exhaustive `match`; results and options, with `try` to pass an error up
  and `defer` for cleanup on every return; function references; generics
  checked once with interfaces and `where` clauses; constants computed at
  compile time; contracts (`req`, `ens`, `inv`) checked in every backend;
  modules and imports; threads and atomics, and `std/par` for parallel maps
  and reductions without `unsafe`.
- **Three ways to run a program,** which must agree: an interpreter for
  quick runs, WebAssembly with WASI (files, stdio, the command line, the
  environment, clocks, threads), and native Linux x86-64 executables with no
  runtime (`word_count` is 94 KB). Native executables run on any kernel since
  3.17; where `openat2` is missing (before Linux 5.6, Rosetta 2) a program
  opens files with `openat`, and one built with `--sandbox`, which needs
  `openat2` to stay confined, says so at start and exits.
- **A standard library** with printing and file I/O, `"x = {x}"` formatting
  (built into the language), strings and UTF-8 text, streams, JSON, CSV,
  statistics (Python's, to the bit), regular expressions (linear-time, as
  Python's `re` matches), hashes (CRC-32, MD5, SHA-1, SHA-256, SHA-512,
  HMAC), DEFLATE, zlib, and gzip, URLs and HTTP messages, big integers, UTC
  dates, vectors, hash maps, sets, command-line options, small math (min,
  max, floor division, gcd, rounding), and more
  (`aipl doc --find` searches it). Memory you free comes from an allocator
  you pass explicitly, as in Zig: a general-purpose heap that stops the
  program on a double free, an arena, or one you write.
- **A self-hosted toolchain.** The resolver, type checker, code generator,
  and native backend are written in AIPL, and produce the same bytes and the
  same error messages as the Rust implementation. The `aipl` command the
  installer builds is still the Rust one: the interpreter and the
  WebAssembly runner are Rust, and other platforms have no seed yet.

**Memory safety.** Outside `(unsafe ...)`, every memory access is checked, in
the interpreter and in compiled code: array indexes against the length,
pointers for null and for their type, and use after free (a pointer kept past
a free stops the program with `Use after free` and the line, even after the
memory holds a new object; [docs/design/GENERATIONS_PLAN.md](docs/design/GENERATIONS_PLAN.md)).
The limits: AIPL does not check for data races, so threads need `unsafe` (but
`std/par` runs work in parallel without it); a fast arena, which a program
chooses by name, catches a stale pointer only once its memory is reused; and
a pointer made by `ptr.cast` is not checked. The checks cost little: six of
the eight benchmarks take the time they took without them or less.

**Speed.** It is not as fast as C. On the eight benchmarks, native code takes
1.2 to 10.4 times as long as C compiled with `gcc -O2`, and WebAssembly under
wasmtime 1.4 to 7.5 times as long; the slowest are k-nucleotide and pidigits,
where C uses a hand-packed key and GMP. [docs/BENCHMARKS.md](docs/BENCHMARKS.md)
has every number and why.

## What is next

In order ([ROADMAP.md](ROADMAP.md) has the details and everything else that
is missing):

1. **Launch readiness** ([docs/LAUNCH_CHECKLIST.md](docs/LAUNCH_CHECKLIST.md)):
   the experiment measuring how well agents write AIPL (the A/B harness in
   [tools/harness/](tools/harness/) is built; its results come first), the
   macOS (Apple Silicon) port, and the final checks.
2. **The standard library:** networking and starting processes, once WASI
   can, and time zones.
3. **Rust out of the build path:** the seed builds the compiler on Linux
   x86-64 today; next, a WebAssembly seed for other platforms and an
   installer that needs no Rust. The Rust compiler stays as the referee.
4. **Packages:** a registry with versioned packages, dependency resolution,
   and lock files.

## How it is made and tested

AIPL is designed by Matt Warpinski and implemented with AI coding agents.
Nothing is taken on trust: every change has to pass checks that compare
independent implementations against each other.

- **506 Rust tests** and an AIPL test suite, run on every push by CI.
- **The interpreter, WebAssembly, and native code must agree.** Differential
  tests run the same programs every way, and a fuzzer generates random
  well-typed programs and compares their output and failures (CI runs 500
  per push). It has found and fixed real bugs.
- **Two compilers, byte for byte.** The Rust and the AIPL toolchains must
  produce identical output; a second fuzzer mutates programs and compares
  the two (3,000 per push).
- **Tests that are themselves checked.** New tests are checked by planting
  bugs in the code under test and confirming the tests catch them.
- **The documents are checked too:** `tools/docs_check.py` (in CI) fails on a
  file, function, flag, or number a document names that is not true.

## Documentation

- [docs/TOUR.md](docs/TOUR.md): a ten-minute tour of the language, for
  people.
- [docs/FAQ.md](docs/FAQ.md): why S-expressions, why so verbose, why not an
  existing language, and the other questions people ask first.
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
