# The stage-0 compiler

`aipl-linux-x86_64` is the AIPL compiler as a native Linux x86-64
executable (and `aipl-linux-aarch64` as an AArch64 one: see below): `aipl_src/aipl.aipl`, which joins the self-hosted toolchain
(`driver.aipl`: resolve, type-check, compile to WebAssembly) and the
native backend (`native/native.aipl`: WebAssembly to machine code). It is
statically linked, needs nothing but the Linux kernel, and builds the next
version of itself:

```bash
sh bootstrap/build.sh            # about 8 seconds; the result is target/bootstrap/aipl
```

On Linux AArch64 the script uses `aipl-linux-aarch64` instead (see "On AArch64 Linux" below).

The script has the seed build the current source (stage 1), stage 1 build
it again (stage 2), and stage 2 build itself once more, which must give
the same bytes (a fixed point). No Rust is involved. The resulting `aipl`
does what `aipl compile`, `aipl verify`, `aipl test`, `aipl watch`, `aipl serve
--fresh`, and `aipl eval` do in the Rust command (`test`, `watch`, and `eval` run
programs natively, not in the VM),
and has `aipl run`, which runs a source file or a `.wasm` module natively (the Rust
command has it for a `.wasm` file only, under wasmtime):

```bash
target/bootstrap/aipl compile FILE.aipl -o OUT.wasm
target/bootstrap/aipl compile --exe [--sandbox] FILE.aipl -o OUT
target/bootstrap/aipl verify FILE.aipl    # resolve and type-check, compile nothing
target/bootstrap/aipl run [FLAGS] FILE.aipl|FILE.wasm [ARGS]...   # build into a temporary file, run it, remove it
target/bootstrap/aipl test FILE|DIR [-k TEXT]           # each (test ...) in a fresh process, natively
target/bootstrap/aipl watch FILE [--test] [-- ARGS...]  # run again when a file it reads changes
target/bootstrap/aipl serve --fresh FILE --listen ADDR:PORT [--requests N]   # a fresh program per connection
target/bootstrap/aipl eval -e EXPRS [--with FILE]       # a value and its type, natively; eval FILE runs it as run does
```

The command is built with `--allow-run '*'` (every stage does, and so does
the by-hand build below): a native executable may start only the programs it
was granted, and `run` starts whatever it has just built. A build without the
grant works as before, and its `run` says why it cannot start anything.

It finds the standard library at `$AIPL_STD`, else `share/aipl/std/`
beside its folder (an install or a kit, `<prefix>/bin/aipl` with
`<prefix>/share/aipl/std/`), else `aipl_src/std/` under the working
directory (aipl_src/stdpath.aipl). An executable it writes is executable: it
sets the bit itself (`fs.chmod`, over `aipl_sys_v1`; WASI has no call for it),
so no `chmod +x` is needed.

**By hand**, each executable builds the next:

```bash
./bootstrap/aipl-linux-x86_64 compile --exe --allow-run '*' aipl_src/aipl.aipl -o target/aipl1
./target/aipl1 compile --exe --allow-run '*' aipl_src/aipl.aipl -o target/aipl2
cmp target/aipl1 target/aipl2       # no output: the compiler reproduces itself
```

**The first executable** of all was built by the Rust toolchain, which is how
the seed was made (`cargo build --release`, then
`./target/release/aipl compile --exe --allow-run '*' aipl_src/aipl.aipl -o target/aipl0`).

The rest of the Rust command (`eval`, `test`, and `watch` in the VM) is not here: the
interpreter is Rust, and running WebAssembly takes wasmtime. Native
executables run directly.

## On AArch64 Linux

The compiler also builds for AArch64: `aipl compile --exe --target arm64 --allow-run '*'
aipl_src/aipl.aipl -o stage1` (the default `--target` on an AArch64 machine). Run on AArch64
Linux, `stage1` compiles the compiler again to the same bytes (a fixed point) and every program
in `examples/`, `benchmarks/`, and `tests/aipl` to the bytes the Rust command makes, with no
Rust and no x86-64 machine on the second stage. `sh tools/arm64_bootstrap.sh HOST` checks it all
over ssh on an Apple Silicon Mac with Docker (3 minutes).

`aipl-linux-aarch64` is that compiler as a Linux AArch64 executable, the seed there: on an
AArch64 Linux machine `sh bootstrap/build.sh` has it build the source (stage 1), stage 1
build it again (stage 2), and stage 2 once more (a fixed point), as on x86-64, with no Rust.
It is larger than the x86-64 seed (7.6 MB against 4 MB) because the AArch64 backend is a
baseline one, every value in a stack slot (docs/design/ARM64_PLAN.md). The process operations
(procs_arm64.aipl) are built for AArch64, so the `aipl` the script makes does what the x86-64 one
does except serve and listen: `compile`, `verify`, `run`, `test`, `watch`, `eval`, and `doc`.
Sockets (`aipl serve`, `--listen`, `--serve`), threads, and `--sandbox` are not built for AArch64
yet. The seed itself is from before the process operations, so it cannot start a program (its
stage 1 cannot, and stage 2 can): `build.sh`'s result is the compiler that can, and it is the
bytes `tools/arm64_bootstrap.sh` finds the Rust-built compiler to be.

## Why the Rust compiler stays

A compiler built by an earlier binary of itself could carry something the
source does not say: a bug, or something planted, that copies itself
into each new version (Ken Thompson, "Reflections on Trusting Trust",
1984). Reading the source cannot rule that out. Building the same source
with an independent compiler can (David A. Wheeler's "diverse double
compiling"), and AIPL has one: the Rust toolchain.

`tests/test_bootstrap.rs` checks it on every `cargo test`: stage 2, which
comes from the seed, is byte for byte the executable the Rust toolchain
builds from the same source. If the seed carried anything the source does
not, stage 1 would pass it on and the two would differ. The same test
checks that the AIPL command compiles every example, benchmark, and test
program to the same bytes as the Rust command, with the same errors.

## Where this seed came from

Built 2026-10-29 by the Rust toolchain, from the source of commit
5d08411, with `--allow-run '*'`, so that the seed is the whole command it
can be: `verify`, `run`, `test`, and `watch` as well as `compile` and `doc`
(docs/design/aipl_standalone.md, S2). The compiler's source now uses `sys.poll`
(`std/time.sleep`, for `watch`), which the seed before could not read, so this
refresh was needed for `bootstrap/build.sh` to work at all (the owner's rule: a
seed is refreshed for an immediate need only):

```bash
cargo build --release
./target/release/aipl compile --exe --allow-run '*' aipl_src/aipl.aipl -o bootstrap/aipl-linux-x86_64
```

sha256 `9e5f706b62a6e63745187b5fff9bf197d062686d48d6bed6d7f09d95ef6f68a1`.
Building that commit's source with the seed gives the same bytes (stage
2 is the seed), so for now the seed is its own stage 2.

Earlier seeds: the one built 2026-10-29 from 0ca006e (sha256
`c6bed76f2347d9b89dde3b366d89530b3f25c79cac801ca8f82c95ffdf441d53`), for
`std/fs`'s `mkdir` and `rmdir`; the one built 2026-10-29 from 49ec350 (sha256
`3717ed4a7be2cfc87c871e2d05741ad2213321930d253f73b582b073e4331051`), for
`std/fs`'s `rename` and the compiler writing its output beside it and renaming it into place; the one built 2026-10-29 from 20888dd (sha256
`1ba9a19ebfc1bda2614f205a783ce95bb6c612e91ff700b25e4d995f2f251e9b`), for
`fs.chmod` and the process operations, which set the output's execute bit; the one built 2026-10-27 from 01bc216 (sha256
`ae30b56410a1f5254422ff280e48b22a50334328d9ff04b434f341d086a6b4be`), for
the `openat` fallback where `openat2` fails (Linux before 5.6, Rosetta 2, a
container profile that refuses it); the one built 2026-10-27 from 29416a9 (sha256
`840060d9a7d52bd6a6ca402b5881f791110fa68f9a8386306ef70c52cbe6a0bc`), for
Task D's read-only first segment; the one built 2026-10-26 from bb8c35f (sha256
`ebc67e10ca127d78e4a03dc7c735ee7a4e48c853af8baa2511f9c49f92b4d5c5`), so
that the compiler's own source can give a struct's fields by name,
`(new S a [x:1 y:2])` and `(make V [x:1 y:2])`, and write a zero as
`(zero T)` (docs/design/LANGUAGE_DECISIONS.md item 4); the one built
2026-10-20 from 59e3286 (sha256
`65b27359549178d8d2faec8cecc42f945fde1e241ed46b55b1c18d294e46d01a`), so
that the compiler's imports can write `(no_leak)` in a function type; the
one built 2026-10-19 from 7a55d99 (sha256
`cd16eff8d957f7733ad903b23110c3dc86ecefb75e5df09500ba9b9bee7dd593`), for
`(new S a)` and `(arr.new T a n)`, which name their allocator (decision
8); the one built that day from 236220e (sha256
`f04d9082e75f9f1eceed897f720601bd0b6fc0dbb5c65bf191f771f27785e59e`), for
the `(no_leak)` clause; the one built that day from 61ca2b6 (sha256
`e2f2b1f53e31cf246b989ec330e75c277f0a19f2d3f69c3113147569f9453169`), for
storage types (`(arr u8)`, which the checked views need), `when`, `for`,
`and`/`or` of more than two, the formatting forms, `dbg`, and `(test
...)`; and the seed of 2026-10-16 (sha256
`72a36ea2bdd10795c572916dd5f75ebbd3360984c1b8f841b61729edbe22e923`),
built from the commit that added generation checks, whose source first
used `ptr.make`.

## Where the AArch64 seed came from

Built 2026-10-30 by the Rust toolchain, from the source of commit ac88301, with
`--allow-run '*'`:

```bash
cargo build --release
./target/release/aipl compile --exe --target arm64 --allow-run '*' aipl_src/aipl.aipl -o bootstrap/aipl-linux-aarch64
```

AArch64 seed sha256 `8554dc536675403a6ee236d7a882179e32e90786526938af5bd2df1e0863ab76`.
The self-hosted x86-64 command builds the same bytes, and so does the seed itself on AArch64 Linux
(at that commit stage 2 was the seed; the process operations since make later stages differ from
it, as they should: `tools/arm64_bootstrap.sh` checks that the seed's chain builds the Rust-built
compiler); `tests/test_bootstrap.rs` runs
the seed's stages on AArch64 Linux as it does on x86-64.

## Replacing the seed

The seed must be able to compile the compiler's current source. So the
compiler's own source may use only what the seed's version of the
language has: a new feature is added first, without being used in
`aipl_src/`, and used there only after the seed is replaced. When
`cargo test` reports that the seed cannot build the source, or at a
release, replace it with the build above and update the date, the commit,
and the hash here. Replace it rarely, since every version stays in the
repository's history (about 3 MB each).
