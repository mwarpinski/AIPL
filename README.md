# AIPL

[![CI](https://github.com/mwarpinski/AIPL/actions/workflows/ci.yml/badge.svg)](https://github.com/mwarpinski/AIPL/actions/workflows/ci.yml)

AIPL is a small, statically typed systems language meant to be written by
AI coding agents. It compiles to WebAssembly and to native Linux executables
(x86-64 and AArch64), and its compiler can compile itself.

**Status: early, and honest about it.** The language, the standard library,
and both compilers work and are heavily tested ([what works](#what-works-today),
[how it is tested](#how-it-is-made-and-tested)). Native executables are Linux
only (x86-64 and AArch64); WebAssembly runs wherever wasmtime does, and on Windows `aipl compile --exe`
makes a single file that bundles the launcher. CI tests Linux, on x86-64 and arm64, and builds and
tests Windows on x86-64 (the AIPL suite, processes, sockets, and the installer); an Apple Silicon Mac is
checked by hand.
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
  at main (gcd.aipl:18:14)
```

A type mistake, such as `(call gcd 10 true)` on line 18, is caught before
the program runs (arguments are counted from 0):

```
$ aipl verify bad.aipl
Error: bad.aipl: 18:14: Arg 1 of 'gcd' expects i32, got bool
```

## Install

On Linux (x86-64 or arm64) and macOS (Apple silicon), one command installs a
prebuilt AIPL. It needs only `curl` and `tar`: no git, no Rust, and nothing
runs as root. It downloads the release for your machine, checks it against its
published SHA-256, and unpacks it into `~/.aipl` (the programs in `bin`: on
Linux `aipl`, the compiler written in AIPL itself, and `aipl-net`, a small
helper for host names and HTTPS; on macOS `aipl` and `aipl-run`; and the
README, specification, examples, and on Linux the standard library in
`share`), and puts `~/.aipl/bin` on your `PATH`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh | sh
```

To read it before running it (it is about 300 lines of POSIX `sh`):

```bash
curl --proto '=https' --tlsv1.2 -sSfO https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh
less install.sh && sh install.sh
```

`sh install.sh --help` lists the options (`--version` to install a release
other than the latest, `--no-modify-path`); `sh install.sh --uninstall`
removes it. Run the installer again to update. Each release's files are also on
the [releases page](https://github.com/mwarpinski/AIPL/releases) to download by hand.

**On Windows 10 or 11 (x86-64)**, in PowerShell, `install.ps1` does the same: it
needs no git, no Rust, no Visual C++ runtime, and no administrator rights, downloads the
release zip, checks its SHA-256, unpacks it into `%USERPROFILE%\.aipl`, and adds
`%USERPROFILE%\.aipl\bin` to your user `PATH` (open a new terminal to see it):

```powershell
irm https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.ps1 | iex
```

It takes its options from environment variables (`AIPL_VERSION`, `AIPL_HOME`,
`AIPL_NO_PATH=1`; `$env:AIPL_UNINSTALL = 1` removes it); `install.ps1` itself lists them.
What differs on Windows: a program sees the root of the drive it runs from as `/`,
and reaches no other drive, so keep what it reads and writes on that drive and write
`/Users/me/notes.txt` or a relative path; an argument you give it that is an absolute
path on that drive (`C:\Users\me\notes.txt`) is turned into that for you. (That goes
for the compiler too: run `aipl compile` from the drive the source is on; when that is not
the drive the standard library is unpacked to, it is unpacked into `.aipl/` there.) Starting
other programs (`std/proc`) works, but a signal there ends a process at once (status
256 plus the signal), and there are no native executables (`compile --exe` bundles
the launcher).

To build from source instead, you need git, a stable Rust toolchain
([rustup.rs](https://rustup.rs)), and a C compiler for linking (e.g.
`build-essential`). `sh install.sh --from-source` (or `--ref` with a branch,
tag, or commit) does it for you, into `~/.aipl/src`, and a machine with no
prebuilt release gets this automatically. Or build a clone by hand:

```bash
git clone https://github.com/mwarpinski/AIPL.git && cd AIPL
cargo build --release
```

This builds `target/release/aipl` (the compiler, interpreter, and checker)
and `target/release/aipl-run` (the launcher for compiled WebAssembly). Either
way, `aipl` carries the standard library inside it, so a copied or installed
one needs nothing beside it but `aipl-run` (run from a clone's `target/`, it
reads that clone's `aipl_src/`, so an edit there shows at once).

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

From the examples directory (`~/.aipl/share` after the installer, or a clone):

```bash
aipl verify examples/word_count.aipl            # parse and type-check
aipl doc text                                   # a module's functions and types, with their comments
aipl doc --find split                           # search the standard library
aipl test rows.aipl                             # run a file's (test ...) items, each alone
aipl watch scratch.aipl                         # run it again on every save
aipl eval -e '(call str.find (call str.from_str "a,b") (call str.from_str ","))'   # prints 1 : i32
aipl compile examples/word_count.aipl -o wc.wasm
aipl run examples/word_count.aipl README.md       # compile and run it (or: aipl run wc.wasm README.md)
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
native executable in about 3 seconds (on Linux AArch64, which has its own
seed and a code generator with an optimizer, about 5). Without Rust, start from the
checked-in seed for your machine. From the source directory, this script builds the compiler
with the seed, builds it again with the result, and checks that a third build
is identical (about 8 seconds):

```bash
sh bootstrap/build.sh               # the result is target/bootstrap/aipl
```

The AIPL-built compiler does `compile` (to `.wasm`, or with `--exe` to an
executable), `verify`, `doc`, `run`, `test`, `watch`, `eval`, and `serve
--fresh` without Rust at run time (a `.wasm` file is run natively on Linux
x86-64; elsewhere, and in the interpreter for `--vm`, the Rust command is
still needed). It compiles every program to the same bytes as the Rust
one, which stays as the referee: `cargo test` checks the seed's build byte for
byte against it (on AArch64 too, in CI). [bootstrap/README.md](bootstrap/README.md) has the other
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
  environment, clocks, threads), and native Linux executables, x86-64 and
  AArch64, with no runtime (`word_count` is 94 KB on x86-64). Native executables run on any kernel since
  3.17; where `openat2` is missing (before Linux 5.6, Rosetta 2) a program
  opens files with `openat`, and one built with `--sandbox`, which needs
  `openat2` to stay confined, says so at start and exits.
- **A standard library** with printing and file I/O, `"x = {x}"` formatting
  (built into the language), strings and UTF-8 text, streams, JSON, CSV,
  statistics (Python's, to the bit), regular expressions (linear-time, as
  Python's `re` matches), hashes (CRC-32, MD5, SHA-1, SHA-256, SHA-512,
  HMAC), DEFLATE, zlib, and gzip, URLs and HTTP messages, TCP sockets with an
  HTTP server and client (HTTPS and host names through Rust's: `aipl eval`,
  aipl-run, `--target wasm` bundles, and in a native executable the `aipl-net`
  helper, which reaches only the hosts the program was granted), starting, signalling, and
  timing out other programs, big integers, UTC dates, vectors, hash maps,
  sets, command-line options, math (min, max, floor division, gcd, rounding,
  and exp, ln, pow, the trigonometric and hyperbolic functions, written in
  AIPL as fdlibm computes them), and more
  (`aipl doc --find` searches it). Memory you free comes from an allocator
  you pass explicitly, as in Zig: a general-purpose heap that stops the
  program on a double free, an arena, or one you write.
- **A self-hosted toolchain.** The resolver, type checker, code generator,
  and native backend are written in AIPL, and produce the same bytes and the
  same error messages as the Rust implementation. The `aipl` command the
  installer builds is still the Rust one: the interpreter and the
  WebAssembly runner are Rust, and macOS has no seed yet (Linux x86-64 and AArch64 do).

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

[ROADMAP.md](ROADMAP.md) has the whole list, in the order a newcomer meets it. The big items:

1. **The first hour:** `aipl run` the same on every platform, editor support (a grammar, then a language server), better
   error messages and a REPL, and a package story.
2. **Platform parity:** native executables on macOS and Windows (today they bundle a launcher), and the self-hosted
   `aipl` there, so every platform installs the same command Linux does.
3. **A stability promise:** a language version and a deprecation policy, which packages need too.
4. **Release hygiene:** signed releases, Homebrew/winget/apt packages, and the aipl.sh website.

## How it is made and tested

AIPL is designed by Matt Warpinski and implemented with AI coding agents.
Nothing is taken on trust: every change has to pass checks that compare
independent implementations against each other.

- **551 Rust tests** and an AIPL test suite, run on every push by CI.
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
- [docs/](docs/README.md): the index of everything else: benchmarks, the launch
  checklist, design records of the larger pieces of work, history, and notes.

## License

MIT ([LICENSE](LICENSE)).
