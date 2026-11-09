# Roadmap

Where AIPL is going, written 2026-11-08, before the launch. [AIPL_SPEC.md](AIPL_SPEC.md) describes what it does;
[DEVELOPING.md](DEVELOPING.md) has how to build and test it; [docs/README.md](docs/README.md) indexes everything else
(plans under docs/design/, what was done when under docs/history/). When something here gets built, delete its entry
rather than appending a note.

## Where it stands

The launch release is v0.3.0 and carries everything on `main` (docs/LAUNCH_CHECKLIST.md). One-liners install
it on Linux (x86-64 and AArch64), macOS (Apple silicon) and Windows (x86-64).

- **Linux ships the self-hosted `aipl`**: written in AIPL, built from the checked-in seed with no Rust. Its native
  executables (x86-64 and AArch64) come from a backend written in AIPL. Names and HTTPS go through the small Rust helper
  `aipl-net`, which enforces per-host grants (docs/design/SHIP_SELFHOSTED_PLAN.md).
- **macOS and Windows are a preview and ship the Rust `aipl` and `aipl-run`** (wasmtime); `compile --exe` bundles the
  launcher (15 to 20 MB).
- **The Rust toolchain is the referee** (`referee/`): development CI requires both compilers to agree, and the release
  never builds it on Linux. It is also the build for macOS and Windows until they have the self-hosted one.
- **The language** is versioned in design but not yet in promise (item 9), and the FAQ says it is not ready for real use.

The goal of what follows is for AIPL to feel like a normal programming language: install it, write a file, run it, get
good errors, use an editor, add a dependency, ship a binary, and rely on it not to change under you. The list is ordered
by how soon a newcomer meets each item. Sizes are rough: **S** days, **M** a week or two, **L** weeks, **XL** a quarter.

## 1. The first hour

1. **A language server, and the editor packages.** *M, then L.* The TextMate and tree-sitter grammars and the VS Code
   extension are built (the aipl-editors repository); they are not yet published to the VS Code Marketplace and Open VSX.
   Left: a language server over what already exists (`aipl verify` for diagnostics, `aipl doc` for hover, the resolver for
   go-to-definition), which can be written in AIPL.
2. **`aipl repl` in the self-hosted `aipl`.** *S.* The Rust `aipl` has it (referee/src/repl.rs); the self-hosted one does
   not (it needs a way to tell a terminal from a pipe, and to run an expression in the process it is). A REPL is for
   people: an AI has `aipl eval -e`, `verify`, `test` and `watch`.
3. **A package story.** *XL.* Versioned packages, dependency resolution, lock files, and a registry; the command is open
   (`aipl add`, `aipl build`). It needs language versioning (item 9) first. Imports today resolve by filename through the
   search path.

## 2. Platform parity

4. **Native executables on macOS and Windows.** *XL.* A Mach-O backend and a PE backend for x86-64 and AArch64, written in
   AIPL like the ELF ones, so `compile --exe` makes a small executable (tens of KB to a few MB) and not a 15 to 20 MB
   launcher bundle. The Mac mini (`ssh pennylane`, arm64) and the Windows VMs are the test machines.
5. **The self-hosted `aipl` off Linux.** *L, after 4.* Then macOS and Windows install the same command Linux does, and the
   Rust build is only the referee. Until then the two platforms behave differently in small ways.
6. **Windows gaps.** *M.* A program sees only the working directory's drive (WASI preopens cannot cross drives without a
   trampoline or a code generator change); there is no Windows ARM64 build; tests run on one VM type.
7. **Install-test Linux AArch64 on real hardware.** *S.* CI builds it and the installer runs in an arm64 container; nobody has run
   the one-liner on a physical machine of that kind.
8. **Older kernels, and `openat2`'s retry.** *S.* Native executables confine files with `openat2` (Linux 5.6). Where it is
    missing they fall back to plain `openat` and `--sandbox` refuses to start; say so visibly (a warning when compiling).
    Where it is there, the runtime (`emit_openat2` in aipl_src/native/wasi.aipl and wasi_arm64.aipl) never retries
    `EAGAIN`, which the kernel gives a path with a `..` component (RESOLVE_BENEATH cannot prove it stays beneath) when a
    file is renamed anywhere at that moment, as `openat2(2)` says to retry. Measured: with four threads renaming files, 3%
    of opens of `a/b/../f` failed and none of `a/f`. A program that opens a path with `..` on a busy machine can fail a
    read or a write now and then; it was the cause of two unexplained test failures (`cannot read entry file`,
    `cannot write`), which the tests now avoid by using paths without `..`. The fix is a bounded retry in both runtimes,
    which changes the machine code every program embeds, so it comes with a seed refresh (both seeds) and a regression
    test that opens such a path while another thread renames.

## 3. Language and runtime

9. **A stability promise.** *M.* A language version in modules and in the output, a deprecation policy, and tests that
    catch a breaking change. This is also what makes packages (item 3) possible. Until then the FAQ's "not stable" stays.
10. **Close the Rust-only capabilities.** *XL.* Owner principle: a capability only Rust has is a gap in AIPL
    ([docs/design/aipl_standalone.md](docs/design/aipl_standalone.md)). Left: the VM in AIPL (S4; `eval -f`,
    `test --vm`, `--all-backends`), `compile --target wasm` and `--self` from the self-hosted command, and running `.wasm`
    without wasmtime off Linux. The Linux `aipl` drops these modes for now; they remain in the Rust CLI.
11. **TLS and the Rust components.** *L.* No TLS is written in AIPL (hand-written cryptography is not fit for real traffic
    without an audit). `aipl-net` is the first Rust component an AIPL program imports; the general way to import one
    (docs/design/NET_PLAN.md section 7) is open, and `aipl-net` is Linux-only until the Mach-O and PE backends exist.
12. **Concurrency.** *L.* Threads and atomics need `unsafe`; `std/par` is safe but narrow (workers share only plain values).
    Wanted: channels, condition variables, thread-local storage, and a safe way to share data between threads.
13. **Performance.** *L.* Native code takes 1.2 to 10.4 times as long as gcc -O2 (docs/BENCHMARKS.md); the single-pass
    backend is within 0.8 to 2.0 times wasmtime. The weak spots: hashing byte strings (k-nucleotide), big integers
    (pidigits), and a register allocator.
14. **Standard library breadth.** *M, ongoing.* Audit against what a normal language ships. Known gaps: time zones,
    shortest-digits float printing outside JSON, a faster small-block path in `std/heap`. Present today: text, JSON, CSV, regex, hashing, deflate, URLs, HTTP, sockets, processes, dates in UTC.

## 4. Release and project hygiene

15. **Notarising and code signing.** *M.* Releases carry a Sigstore signature over their checksums (docs/RELEASING.md).
    Still to do: notarise the macOS build so Gatekeeper does not warn, and code-sign the Windows executables.
16. **Package-manager installs.** *M.* Homebrew, winget, apt and the AUR, built from the release files. The curl one-liner
    stays, but it is not what people trust.
17. **The aipl.sh website.** *M. Not started: only when asked.* The install commands, the tour, the language reference, a
    playground (the compiler is wasm already).
18. **A wider compatibility matrix.** *M.* Tested today: the Linux `aipl` on Ubuntu 22.04 and 24.04, Debian 11 and 12 and Alpine (it is
    a static executable), one Mac, one Windows 11 build (an Insider preview). Add macOS x86-64 and a stable Windows 11 and 10.

## What the language does not do (by design, or not yet)

Re-checked 2026-11-08 against the self-hosted `aipl` and the specification, line by line.

- **No closures, by design.** `(ref f)` and `call_ref` refer to named functions; a callback's data is a typed context passed
  as its first argument (AIPL_SPEC.md 4.G).
- **Generics are explicit**: every use names its type arguments, and every call of an interface's function names the interface
  and the type. Not yet: interfaces with several type parameters, generic implementations, default functions in an interface
  (AIPL_SPEC.md 4.H).
- **Structs are behind pointers unless declared `(value)`** (at most 16 scalars, copied, never changed: docs/LIMITS.md). A
  heap struct cannot be nested inside another by value (`'A' is a struct, which is only used through a pointer`).
- **No module-level variables, by design**: state is a struct passed to every function. Constants may be computed from others.
- **8- and 16-bit types are storage only**: struct fields and array elements, with no arithmetic at those widths; a read
  gives an `i32`.
- **Unions are immutable values**, built only by `make`; `match` binds one level of fields (no nested patterns).
- **Numeric gaps**: not every conversion pair among `f32`, `f64`, `i32`, `i64` (the unsigned ones are `i64.extend_u` and the
  `divu`-style operations; there is no unsigned-to-float), no float loads or stores on raw memory (floats live in struct
  fields and `(arr f64)`), and loop bounds and addresses are `i32`. Programs are limited to 2 GiB (below).
- **No string operators**: `str.concat`, `str.same`, `(cmp.Ord.compare str)`. Three kinds of text (`str`, `str.Bytes`,
  `buf.Buf`) converted by calling a function.
- **A result or an option in statement position is a compile error**, not a silent discard: use it (`match_result`, `try`)
  or write `(drop e)`. The searches (`str.find`, `vec.index_of`) return an option.
- **Only an allocator that frees, frees.** `mem.alloc` and `alloc.forever` come from a bump cursor that only grows, and a
  `str` made at run time (`format`, `str.concat`) is never freed. `std/heap`, `std/arena` and `std/alloc` are the allocators
  that free (docs/design/HEAP_PLAN.md); `(no_leak)` makes the checker hold a function to it.
- **Memory-safe outside `unsafe`, with stated limits**: no data-race checking, `ptr.cast` pointers are unchecked, a stale
  pointer into a reused arena is caught except by a 1 in 2^32 coincidence, and a fast arena catches one only once its memory
  is reused (docs/FAQ.md, "Is it memory-safe?").
- **Memory caps at 2 GiB** (32768 pages), or at `--max-memory`'s.
- **Contracts are checked at run time, never proven**; a compiled contract message shows no float or string values.
- **Diagnostics report the first error of each definition**, up to 20 (AIPL_SPEC.md 6.1): a syntax, import or constant
  error stops at once, and a function body is checked only when every definition holds.

## Later ideas

- **A 64-bit target** (decided 2026-10-19): the same source compiled for wasm's memory64, as a second target and not a second
  language. Wanted when a program needs more than 2 GiB; an address-sized integer type comes with it, not before.
  `--max-memory` comes first.
- **AI-SQL** (an idea, 2026-10-19): a small, typed SQL written in AIPL, starting as a CSV query tool whose answers are
  checked against SQLite's on the same data. The schedule is set by earning trust in the answers, not by writing the code.
  It would pull focus before the items above are done.
- **Secret types**: a qualifier the checker enforces (as it proves `(no_leak)`) so a secret value never decides a branch, an
  index, a loop count or a division, which would make constant time a property the compiler proves. Useful on its own.
- **A PDF reader or a browser in AIPL** as the longer-term "real systems program"; a PDF reader needs only binary parsing,
  rasterising into memory and file I/O, all of which compile today. A browser also needs a windowing host.
- **Explicitly not now**: SIMD, exceptions (results cover errors), TLS written in AIPL (item 11).
