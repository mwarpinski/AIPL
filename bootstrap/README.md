# The stage-0 compiler

`aipl-linux-x86_64` is the AIPL compiler as a native Linux x86-64
executable: `aipl_src/aipl.aipl`, which joins the self-hosted toolchain
(`driver.aipl`: resolve, type-check, compile to WebAssembly) and the
native backend (`native/native.aipl`: WebAssembly to machine code). It is
statically linked, needs nothing but the Linux kernel, and builds the next
version of itself:

```bash
sh bootstrap/build.sh            # about 3 seconds; the result is target/bootstrap/aipl
```

The script has the seed build the current source (stage 1), stage 1 build
it again (stage 2), and stage 2 build itself once more, which must give
the same bytes (a fixed point). No Rust is involved. The resulting `aipl`
does what `aipl compile` does in the Rust command:

```bash
target/bootstrap/aipl compile FILE.aipl -o OUT.wasm
target/bootstrap/aipl compile --exe [--sandbox] FILE.aipl -o OUT
```

It finds the standard library at `$AIPL_STD`, else `aipl_src/std/` under
the working directory. WASI has no way to set a file's execute bit, so an
executable it writes needs `chmod +x` (the script does this).

The rest of the Rust command (`eval`, `test`, `run`) is not here: the
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

Built 2026-10-15 by the Rust toolchain, from the source of the commit
that added it:

```bash
cargo build --release
./target/release/aipl compile --exe aipl_src/aipl.aipl -o bootstrap/aipl-linux-x86_64
```

sha256 `d02a2d42d69c9c85433307dc016a5f8d9045d6db83115da403c4551cdcbd680a`.
Building that commit's source with the seed gives the same bytes, so for
now the seed is its own stage 2.

## Replacing the seed

The seed must be able to compile the compiler's current source. So the
compiler's own source may use only what the seed's version of the
language has: a new feature is added first, without being used in
`aipl_src/`, and used there only after the seed is replaced. When
`cargo test` reports that the seed cannot build the source, or at a
release, replace it with the build above and update the date, the commit,
and the hash here. Replace it rarely, since every version stays in the
repository's history (2 MB each).
