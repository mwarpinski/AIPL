# Roadmap

Where AIPL is going, written 2026-11-04 after v0.2.0. [AIPL_SPEC.md](AIPL_SPEC.md) describes what it does;
[DEVELOPING.md](DEVELOPING.md) has how to build and test it; [docs/README.md](docs/README.md) indexes everything else
(plans under docs/design/, what was done when under docs/history/). When something here gets built, delete its entry
rather than appending a note.

## Where it stands

v0.2.0 is out. One-liners install it on Linux (x86-64 and AArch64), macOS (arm64) and Windows (x86-64); each was tried
on a clean machine except Linux AArch64, which CI builds and nobody has installed.

- **Linux ships the self-hosted `aipl`**: written in AIPL, built from the checked-in seed with no Rust, 3.0 MB. Its native
  executables (x86-64 and AArch64) come from a backend written in AIPL. Names and HTTPS go through the small Rust helper
  `aipl-net`, which enforces per-host grants (docs/design/SHIP_SELFHOSTED_PLAN.md).
- **macOS and Windows ship the Rust `aipl` and `aipl-run`** (wasmtime); `compile --exe` bundles the launcher (15 to 20 MB).
- **The Rust toolchain is the referee**: tests require both compilers to produce the same bytes. It is also the build
  for macOS and Windows.
- **The language** is stable in design but not in promise: nothing is versioned, and the FAQ says it is not ready for real use.

The goal of what follows is for AIPL to feel like a normal programming language: install it, write a file, run it, get
good errors, use an editor, add a dependency, ship a binary, and rely on it not to change under you. The list is ordered
by how soon a newcomer meets each item. Sizes are rough: **S** days, **M** a week or two, **L** weeks, **XL** a quarter.

## 1. The first hour

1. **`aipl run FILE.aipl` the same everywhere.** *Built, in 0.2.1.* The Rust command now compiles a
   source file and runs it, takes `--no-opt` and `--max-memory`, and takes the program's arguments with or without a `--`,
   as the self-hosted command does (`aipl_run_takes_a_source_file_as_it_takes_a_module`, tests/test_runner.rs). Left:
   compare the two commands' `run` on the whole example corpus in tests/test_bootstrap.rs, as `compile` is.
2. **Editor support.** *M, then L.* A TextMate grammar (VS Code, Zed, Sublime), a tree-sitter grammar (Neovim, Helix, Emacs),
   then a language server over what already exists: `aipl verify` for diagnostics, `aipl doc` for hover, and the resolver
   for go-to-definition. The server can be written in AIPL.
3. **Error messages, and a REPL.** *M.* Audit the messages a newcomer meets first (one error per function today; an `if`
   without an `else` reads `Unexpected token parsing expression: ')'`), report every error in a function, and make
   `aipl eval` with no arguments a REPL.
4. **A package story.** *XL.* Versioned packages, dependency resolution, lock files, and a registry; the command is open
   (`aipl add`, `aipl build`). It needs language versioning (item 11) first. Imports today resolve by filename through the
   search path.
5. **Docs that match the release.** *S.* The FAQ's platform section still says Linux x86-64 is the only tested platform
   with native executables; the README, TOUR and FAQ need one pass against the self-hosted `aipl` and the dropped modes
   (item 12). docs/LAUNCH_CHECKLIST.md is the checklist for it.

## 2. Platform parity

6. **Native executables on macOS and Windows.** *XL.* A Mach-O backend and a PE backend for x86-64 and AArch64, written in
   AIPL like the ELF ones, so `compile --exe` makes a small executable (tens of KB to a few MB) and not a 15 to 20 MB
   launcher bundle. The Mac mini (`ssh pennylane`, arm64) and the Windows VMs are the test machines.
7. **The self-hosted `aipl` off Linux.** *L, after 6.* Then macOS and Windows install the same command Linux does, and the
   Rust build is only the referee. Until then the two platforms behave differently in small ways (item 1 was one).
8. **Windows gaps.** *M.* A program sees only the working directory's drive (WASI preopens cannot cross drives without a
   trampoline or a code generator change); there is no Windows ARM64 build; tests run on one VM type.
9. **Install-test Linux AArch64 on real hardware.** *S.* CI builds and smoke-tests it; nobody has run the one-liner there.
10. **Older kernels.** *S.* Native executables confine files with `openat2` (Linux 5.6). Where it is missing they fall back
    to plain `openat` and `--sandbox` refuses to start; say so visibly (a warning when compiling).

## 3. Language and runtime

11. **A stability promise.** *M.* A language version in modules and in the output, a deprecation policy, and tests that
    catch a breaking change. This is also what makes packages (item 4) possible. Until then the FAQ's "not stable" stays.
12. **Close the Rust-only capabilities.** *XL.* Owner principle: a capability only Rust has is a gap in AIPL
    ([docs/design/aipl_standalone.md](docs/design/aipl_standalone.md)). Left: the VM in AIPL (S4; `eval -f`,
    `test --vm`, `--all-backends`), `compile --target wasm` and `--self` from the self-hosted command, and running `.wasm`
    without wasmtime off Linux. The Linux `aipl` drops these modes for now; they remain in the Rust CLI.
13. **TLS and the Rust components.** *L.* No TLS is written in AIPL (hand-written cryptography is not fit for real traffic
    without an audit). `aipl-net` is the first Rust component an AIPL program imports; the general way to import one
    (docs/design/NET_PLAN.md section 7) is open, and `aipl-net` is Linux-only until the Mach-O and PE backends exist.
14. **Concurrency.** *L.* Threads and atomics need `unsafe`; `std/par` is safe but narrow (workers share only plain values).
    Wanted: channels, condition variables, thread-local storage, and a safe way to share data between threads.
15. **Performance.** *L.* Native code takes 1.2 to 10.4 times as long as gcc -O2 (docs/BENCHMARKS.md); the single-pass
    backend is within 0.8 to 2.0 times wasmtime. The weak spots: hashing byte strings (k-nucleotide), big integers
    (pidigits), and a register allocator.
16. **Standard library breadth.** *M, ongoing.* Audit against what a normal language ships. Known gaps: time zones,
    shortest-digits float printing outside JSON, a faster small-block path in `std/heap`. Present today: text, JSON, CSV, regex, hashing, deflate, URLs, HTTP, sockets, processes, dates in UTC.

## 4. Release and project hygiene

17. **Signing.** *M.* Checksums are published, nothing is signed. Sign the release files, notarise the macOS build so
    Gatekeeper does not warn, and code-sign the Windows executables.
18. **Package-manager installs.** *M.* Homebrew, winget, apt and the AUR, built from the release files. The curl one-liner
    stays, but it is not what people trust.
19. **Release automation.** *S.* A release today needs the version bumped in `Cargo.toml` and in `aipl_src/aipl.aipl` by hand
    and the notes written by hand. Generate both (release.yml already fails when they disagree), and install-test every
    platform leg in CI against the real release files.
20. **The aipl.sh website.** *M. Not started: only when asked.* The install commands, the tour, the language reference, a
    playground (the compiler is wasm already).
21. **A wider compatibility matrix.** *M.* Tested today: one Linux distribution and kernel, one Mac, one Windows 11 build (an
    Insider preview). Add Alpine/musl, an older glibc, macOS x86-64 and a stable Windows 11 and 10.

## What the language does not do (by design, or not yet)

Carried from the audit of 2026-10-27; **not re-checked since**, so treat a specific claim as a lead.

- **No closures, by design.** `(ref f)` and `call_ref` refer to named functions; a callback's data is a typed context passed
  as its first argument (AIPL_SPEC.md 4.G).
- **Generics are explicit**: every use names its type arguments, and every call of an interface's function names the interface
  and the type. Not yet: interfaces with several type parameters, generic implementations, default functions in an interface.
- **Structs are behind pointers unless declared `(value)`** (at most 8 scalars, copied, never changed). A heap struct cannot be
  nested inside another by value.
- **No module-level variables, by design**: state is a struct passed to every function. Constants may be computed from others.
- **8- and 16-bit types are storage only**; there is no arithmetic at those widths; no checked division or shift.
- **Unions are immutable values**, built only by `make`; `match` binds one level of fields (no nested patterns).
- **Numeric gaps**: not every conversion pair among `f32`, `f64`, `i32`, `i64` (no unsigned ones), no float loads or stores on
  raw memory, and loop bounds and addresses are `i32`. Programs are limited to 2 GiB (below).
- **No string operators**: `str.concat`, `str.same`, `(cmp.Ord.compare str)`. Three kinds of text (`str`, `str.Bytes`,
  `buf.Buf`) converted by calling a function.
- **A `result` or an `fs.*` status in statement position is discarded silently** (audit D5); the searches (`str.find`,
  `vec.index_of`) return -1 for not found.
- **Only an allocator that frees, frees.** `mem.alloc` and `alloc.forever` come from a bump cursor that only grows. `std/heap`,
  `std/arena` and `std/alloc` are the allocators that free (docs/design/HEAP_PLAN.md).
- **Memory-safe outside `unsafe`, with stated limits**: no data-race checking, `ptr.cast` pointers are unchecked, a stale
  pointer into a reused arena is caught except by a 1 in 2^32 coincidence, and a fast arena catches one only once its memory
  is reused (docs/FAQ.md, "Is it memory-safe?").
- **Memory caps at 2 GiB** (32768 pages), or at `--max-memory`'s.
- **Contracts are checked at run time, never proven**; a compiled contract message shows no float or string values.
- **Diagnostics report one error per function.**

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
- **Explicitly not now**: SIMD, exceptions (results cover errors), TLS written in AIPL (item 13).
