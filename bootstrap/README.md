# The stage-0 compiler

`aipl-linux-x86_64` is the AIPL compiler as a native Linux x86-64
executable: `aipl_src/aipl.aipl`, which joins the self-hosted toolchain
(`driver.aipl`: resolve, type-check, compile to WebAssembly) and the
native backend (`native/native.aipl`: WebAssembly to machine code). It is
statically linked, needs nothing but the Linux kernel, and builds the next
version of itself:

```bash
sh bootstrap/build.sh            # about 8 seconds; the result is target/bootstrap/aipl
```

The script has the seed build the current source (stage 1), stage 1 build
it again (stage 2), and stage 2 build itself once more, which must give
the same bytes (a fixed point). No Rust is involved. The resulting `aipl`
does what `aipl compile` and `aipl verify` do in the Rust command:

```bash
target/bootstrap/aipl compile FILE.aipl -o OUT.wasm
target/bootstrap/aipl compile --exe [--sandbox] FILE.aipl -o OUT
target/bootstrap/aipl verify FILE.aipl    # resolve and type-check, compile nothing
```

It finds the standard library at `$AIPL_STD`, else `share/aipl/std/`
beside its folder (an install or a kit, `<prefix>/bin/aipl` with
`<prefix>/share/aipl/std/`), else `aipl_src/std/` under the working
directory (aipl_src/stdpath.aipl). An executable it writes is executable: it
sets the bit itself (`fs.chmod`, over `aipl_sys_v1`; WASI has no call for it),
so no `chmod +x` is needed.

**By hand**, each executable builds the next:

```bash
./bootstrap/aipl-linux-x86_64 compile --exe aipl_src/aipl.aipl -o target/aipl1
./target/aipl1 compile --exe aipl_src/aipl.aipl -o target/aipl2
cmp target/aipl1 target/aipl2       # no output: the compiler reproduces itself
```

**The first executable** of all was built by the Rust toolchain, which is how
the seed was made (`cargo build --release`, then
`./target/release/aipl compile --exe aipl_src/aipl.aipl -o target/aipl0`).

The rest of the Rust command (`eval`, `test`, `run`, `watch`, `serve`) is not here: the
interpreter is Rust, and running WebAssembly takes wasmtime. Native
executables run directly.

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
0ca006e, so that the compiler can make and remove directories: `std/fs` has
`mkdir` and `rmdir` (with `remove` and `rename` from the seeds before), which
the compiler imports, and it can now write its output into a directory it
makes (docs/design/aipl_standalone.md, S2):

```bash
cargo build --release
./target/release/aipl compile --exe aipl_src/aipl.aipl -o bootstrap/aipl-linux-x86_64
```

sha256 `c6bed76f2347d9b89dde3b366d89530b3f25c79cac801ca8f82c95ffdf441d53`.
Building that commit's source with the seed gives the same bytes (stage
2 is the seed), so for now the seed is its own stage 2.

Earlier seeds: the one built 2026-10-29 from 49ec350 (sha256
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

## Replacing the seed

The seed must be able to compile the compiler's current source. So the
compiler's own source may use only what the seed's version of the
language has: a new feature is added first, without being used in
`aipl_src/`, and used there only after the seed is replaced. When
`cargo test` reports that the seed cannot build the source, or at a
release, replace it with the build above and update the date, the commit,
and the hash here. Replace it rarely, since every version stays in the
repository's history (about 3 MB each).
